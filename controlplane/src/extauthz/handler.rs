use axum::extract::{Request, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::Router;

use shared::config_types::RouteConfig;

use crate::admin::state::SharedState;

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
async fn check(State(state): State<SharedState>, req: Request) -> impl IntoResponse {
    // 1. Overload protection: reject if at max concurrency.
    let _permit = match state.concurrency_limit.try_acquire() {
        Ok(p) => p,
        Err(_) => {
            state.metrics.record_overload();
            return overload_response();
        }
    };

    state.metrics.inc_inflight();
    let (parts, _body) = req.into_parts();
    let result = check_inner(&state, &parts.headers, parts.uri.path()).await;
    state.metrics.dec_inflight();

    result
}

/// Inner check logic, separated so inflight gauge is always decremented.
async fn check_inner(
    state: &SharedState,
    headers: &HeaderMap,
    path: &str,
) -> axum::response::Response {
    let host = header_str(headers, "host").unwrap_or("*");

    let cs = state.config_state.read().await;
    let cfg = &cs.config;

    // 2. Route matching.
    let route = match match_route(&cfg.routes, host, path) {
        Some(r) => r,
        None => return allow_response(HeaderMap::new()),
    };

    let mut response_headers = HeaderMap::new();

    // 3. Authentication (if required).
    if route.auth_required {
        let token = match extract_bearer(headers) {
            Some(t) => t,
            None => {
                state
                    .metrics
                    .record_auth_failure(&route.name, "missing_token");
                return deny_response(StatusCode::UNAUTHORIZED, "missing_token");
            }
        };

        let provider_name = route.auth_provider.as_deref().unwrap_or("main");
        let provider = match cfg.auth.providers.iter().find(|p| p.name == provider_name) {
            Some(p) => p,
            None => {
                state
                    .metrics
                    .record_auth_failure(&route.name, "unknown_provider");
                return deny_response(StatusCode::SERVICE_UNAVAILABLE, "auth_provider_unavailable");
            }
        };

        let cache = match state.jwks_registry.get(provider_name) {
            Some(c) => c,
            None => {
                state
                    .metrics
                    .record_auth_failure(&route.name, "unknown_provider");
                return deny_response(StatusCode::SERVICE_UNAVAILABLE, "auth_provider_unavailable");
            }
        };

        let required_scopes = route.required_scopes.as_deref().unwrap_or(&[]);
        match crate::auth::validate_with_refresh(token, provider, cache, required_scopes).await {
            Ok(identity) => {
                if let Ok(v) = identity.sub.parse() {
                    response_headers.insert("x-auth-sub", v);
                }
                if let Ok(v) = identity.iss.parse() {
                    response_headers.insert("x-auth-iss", v);
                }
                let scopes = identity.scopes.join(" ");
                if let Ok(v) = scopes.parse() {
                    response_headers.insert("x-auth-scopes", v);
                }
            }
            Err(e) => {
                state
                    .metrics
                    .record_auth_failure(&route.name, e.error_code());
                return deny_response(e.http_status(), e.error_code());
            }
        }
    }

    // 4. Rate limiting.
    let key_value = if route.rate_limit.key_by == "sub" {
        response_headers
            .get("x-auth-sub")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("anonymous")
            .to_owned()
    } else {
        client_ip(headers, cs.config.gateway.trust_forwarded_headers)
    };

    let bucket_key = crate::ratelimit::build_key(
        &cfg.rate_limits.redis_key_prefix,
        &route.name,
        &route.rate_limit.key_by,
        &key_value,
    );

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
            let remaining = decision.remaining.to_string();
            let limit = decision.limit.to_string();
            if let Ok(v) = remaining.parse() {
                response_headers.insert("x-ratelimit-remaining", v);
            }
            if let Ok(v) = limit.parse() {
                response_headers.insert("x-ratelimit-limit", v);
            }
            if let Ok(v) = decision.mode.as_header_value().parse() {
                response_headers.insert("x-ratelimit-mode", v);
            }

            if !decision.allowed {
                state.metrics.record_rate_limit_denied(&route.name);
                let retry_after = (decision.retry_after_ms / 1000).max(1).to_string();
                if let Ok(v) = retry_after.parse() {
                    response_headers.insert("retry-after", v);
                }
                return (StatusCode::TOO_MANY_REQUESTS, response_headers).into_response();
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
            return deny_response(StatusCode::SERVICE_UNAVAILABLE, "rate_limiter_unavailable");
        }
    }

    allow_response(response_headers)
}

/// Match an incoming request to a route by hostname and longest path prefix.
pub(super) fn match_route<'a>(
    routes: &'a [RouteConfig],
    host: &str,
    path: &str,
) -> Option<&'a RouteConfig> {
    routes
        .iter()
        .filter(|r| r.hostnames.iter().any(|h| h == "*" || h == host))
        .filter(|r| path.starts_with(&r.path_prefix))
        .max_by_key(|r| r.path_prefix.len())
}

/// Extract the Bearer token from the Authorization header.
pub(super) fn extract_bearer(headers: &HeaderMap) -> Option<&str> {
    let value = headers.get("authorization")?.to_str().ok()?;
    let token = value.strip_prefix("Bearer ")?;
    if token.is_empty() {
        return None;
    }
    Some(token)
}

/// Extract the client IP, optionally trusting X-Forwarded-For.
fn client_ip(headers: &HeaderMap, trust_forwarded: bool) -> String {
    if trust_forwarded {
        if let Some(xff) = header_str(headers, "x-forwarded-for") {
            if let Some(first) = xff.split(',').next() {
                let ip = first.trim();
                if !ip.is_empty() {
                    return ip.to_owned();
                }
            }
        }
    }
    header_str(headers, "x-envoy-external-address")
        .unwrap_or("0.0.0.0")
        .to_owned()
}

fn header_str<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name)?.to_str().ok()
}

fn overload_response() -> axum::response::Response {
    let mut headers = HeaderMap::new();
    if let Ok(v) = "true".parse() {
        headers.insert("x-gateway-overloaded", v);
    }
    if let Ok(v) = "1".parse() {
        headers.insert("retry-after", v);
    }
    let body = serde_json::json!({"error": "gateway_overloaded"});
    (StatusCode::SERVICE_UNAVAILABLE, headers, axum::Json(body)).into_response()
}

fn allow_response(headers: HeaderMap) -> axum::response::Response {
    (StatusCode::OK, headers).into_response()
}

fn deny_response(status: StatusCode, error_code: &str) -> axum::response::Response {
    let body = serde_json::json!({"error": error_code});
    (status, axum::Json(body)).into_response()
}
