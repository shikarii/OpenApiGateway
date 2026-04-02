use axum::extract::{Request, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::Router;

use crate::admin::state::SharedState;
use crate::observability::generate_request_id;

use super::helpers::*;

/// Build the ext_authz axum router.
///
/// Envoy HTTP ext_authz sends the original method and path to the check
/// service, so we use a fallback handler that matches any request.
pub(crate) fn router(state: SharedState) -> Router {
    Router::new().fallback(check).with_state(state)
}

/// Envoy ext_authz HTTP check handler.
///
/// Evaluates auth, rate-limit, and overload for each inbound request.
/// Returns 200 to allow, or 401/403/429/503 to deny.
async fn check(State(state): State<SharedState>, req: Request) -> axum::response::Response {
    let request_id = generate_request_id();

    // 1. Overload protection: reject if at max concurrency.
    let _permit = match state.concurrency_limit.try_acquire() {
        Ok(p) => p,
        Err(_) => {
            state.metrics.record_overload();
            return overload_response(&request_id);
        }
    };

    state.metrics.inc_inflight();
    let (parts, _body) = req.into_parts();
    let result = check_inner(
        &state,
        &parts.headers,
        parts.method.as_str(),
        parts.uri.path(),
        &request_id,
    )
    .await;
    state.metrics.dec_inflight();

    result
}

/// Inner check logic, separated so inflight gauge is always decremented.
///
/// All metrics recording and access log emission happen here since this
/// function already holds the config read lock.
async fn check_inner(
    state: &SharedState,
    headers: &HeaderMap,
    method: &str,
    path: &str,
    request_id: &str,
) -> axum::response::Response {
    let start = std::time::Instant::now();
    let host = header_str(headers, "host").unwrap_or("*");
    let remote_addr = client_ip(headers, false);

    let cs = state.config_state.read().await;
    let cfg = &cs.config;
    let trust_forwarded = cfg.gateway.trust_forwarded_headers;

    // 2. Route matching.
    let route = match match_route(&cfg.routes, host, path) {
        Some(r) => r,
        None => {
            let resp = allow_response(HeaderMap::new(), request_id);
            emit_log(
                request_id,
                &remote_addr,
                host,
                method,
                path,
                "",
                200,
                &start,
                None,
                "none",
                "",
            );
            state
                .metrics
                .record_request("", method, 200, start.elapsed().as_millis() as f64);
            return resp;
        }
    };

    // 3. Method check: verify the HTTP method is allowed for this route.
    if !route.methods.iter().any(|m| m.eq_ignore_ascii_case(method)) {
        tracing::debug!(route = %route.name, method, "method not allowed");
        return method_denied(
            state,
            request_id,
            &remote_addr,
            host,
            method,
            path,
            route,
            &start,
        );
    }

    let mut response_headers = HeaderMap::new();
    let mut auth_subject: Option<String> = None;

    // 4. Authentication (if required).
    if route.auth_required {
        tracing::info!(route = %route.name, "auth_check");
        let token = match extract_bearer(headers) {
            Some(BearerResult::Valid(t)) => t,
            Some(BearerResult::Malformed) => {
                state
                    .metrics
                    .record_auth_failure(&route.name, "invalid_token_format");
                return early_deny(
                    state,
                    request_id,
                    &remote_addr,
                    host,
                    method,
                    path,
                    route,
                    StatusCode::UNAUTHORIZED,
                    "invalid_token_format",
                    &start,
                );
            }
            None => {
                state
                    .metrics
                    .record_auth_failure(&route.name, "missing_token");
                return early_deny(
                    state,
                    request_id,
                    &remote_addr,
                    host,
                    method,
                    path,
                    route,
                    StatusCode::UNAUTHORIZED,
                    "missing_token",
                    &start,
                );
            }
        };

        let provider_name = route.auth_provider.as_deref().unwrap_or("main");
        let provider = match cfg.auth.providers.iter().find(|p| p.name == provider_name) {
            Some(p) => p,
            None => {
                state
                    .metrics
                    .record_auth_failure(&route.name, "unknown_provider");
                return early_deny(
                    state,
                    request_id,
                    &remote_addr,
                    host,
                    method,
                    path,
                    route,
                    StatusCode::SERVICE_UNAVAILABLE,
                    "auth_provider_unavailable",
                    &start,
                );
            }
        };

        let cache = match state.jwks_registry.get(provider_name) {
            Some(c) => c,
            None => {
                state
                    .metrics
                    .record_auth_failure(&route.name, "unknown_provider");
                return early_deny(
                    state,
                    request_id,
                    &remote_addr,
                    host,
                    method,
                    path,
                    route,
                    StatusCode::SERVICE_UNAVAILABLE,
                    "auth_provider_unavailable",
                    &start,
                );
            }
        };

        let required_scopes = route.required_scopes.as_deref().unwrap_or(&[]);
        match crate::auth::validate_with_refresh(token, provider, cache, required_scopes).await {
            Ok(identity) => {
                auth_subject = Some(identity.sub.clone());
                if let Ok(v) = identity.sub.parse() {
                    response_headers.insert("x-auth-sub", v);
                }
                if let Ok(v) = identity.iss.parse() {
                    response_headers.insert("x-auth-iss", v);
                }
                let scopes = identity.scopes.join(",");
                if let Ok(v) = scopes.parse() {
                    response_headers.insert("x-auth-scopes", v);
                }
            }
            Err(e) => {
                state
                    .metrics
                    .record_auth_failure(&route.name, e.error_code());
                return early_deny(
                    state,
                    request_id,
                    &remote_addr,
                    host,
                    method,
                    path,
                    route,
                    e.http_status(),
                    e.error_code(),
                    &start,
                );
            }
        }
    }

    // 5. Rate limiting.
    tracing::info!(route = %route.name, "rate_limit_check");
    let key_value = if route.rate_limit.key_by == "sub" {
        response_headers
            .get("x-auth-sub")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("anonymous")
            .to_owned()
    } else {
        client_ip(headers, trust_forwarded)
    };

    let bucket_key = crate::ratelimit::build_key(
        &cfg.rate_limits.redis_key_prefix,
        &route.name,
        &route.rate_limit.key_by,
        &key_value,
    );

    let rl_mode;
    match state
        .rate_limiter
        .check(
            &bucket_key,
            route.rate_limit.bucket_capacity,
            route.rate_limit.refill_rate_per_sec,
        )
        .await
    {
        Ok(decision) => {
            rl_mode = decision.mode.as_header_value().to_owned();
            insert_rate_limit_headers(&mut response_headers, &decision);

            if !decision.allowed {
                state.metrics.record_rate_limit_denied(&route.name);
                let retry_after = (decision.retry_after_ms / 1000).max(1).to_string();
                if let Ok(v) = retry_after.parse() {
                    response_headers.insert("retry-after", v);
                }
                let status = StatusCode::TOO_MANY_REQUESTS;
                emit_log(
                    request_id,
                    &remote_addr,
                    host,
                    method,
                    path,
                    &route.name,
                    status.as_u16(),
                    &start,
                    auth_subject.as_deref(),
                    &rl_mode,
                    &route.upstream.service,
                );
                state.metrics.record_request(
                    &route.name,
                    method,
                    status.as_u16(),
                    start.elapsed().as_millis() as f64,
                );
                return (status, response_headers).into_response();
            }

            match decision.mode {
                crate::ratelimit::RateLimitMode::DegradedLocal => {
                    state.metrics.record_rate_limit_degraded(&route.name);
                }
                _ => {
                    state.metrics.record_rate_limit_allowed(&route.name);
                }
            }
        }
        Err(_) => {
            state.metrics.record_rate_limit_denied(&route.name);
            return early_deny(
                state,
                request_id,
                &remote_addr,
                host,
                method,
                path,
                route,
                StatusCode::SERVICE_UNAVAILABLE,
                "rate_limiter_unavailable",
                &start,
            );
        }
    }

    emit_log(
        request_id,
        &remote_addr,
        host,
        method,
        path,
        &route.name,
        200,
        &start,
        auth_subject.as_deref(),
        &rl_mode,
        &route.upstream.service,
    );
    state
        .metrics
        .record_request(&route.name, method, 200, start.elapsed().as_millis() as f64);
    allow_response(response_headers, request_id)
}
