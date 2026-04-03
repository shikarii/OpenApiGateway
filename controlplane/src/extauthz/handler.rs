use axum::extract::{Request, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::Router;
use tracing::Instrument;

use crate::admin::state::SharedState;
use crate::observability::generate_request_id;

use super::auth_step::apply_auth;
use super::helpers::*;
use super::plugin_step::{execute_access_plugins, run_plugin_log, PluginExecutionMeta};

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
    let start = std::time::Instant::now();

    // Create a tracing span (becomes an OTel span when tracing layer is active).
    let span = tracing::info_span!(
        "gateway_request",
        http.method = parts.method.as_str(),
        http.url = parts.uri.path(),
        request_id = request_id.as_str(),
        http.status_code = tracing::field::Empty,
        route = tracing::field::Empty,
        upstream_service = tracing::field::Empty,
        auth_subject = tracing::field::Empty,
        duration_ms = tracing::field::Empty,
    );

    // Set inbound traceparent as parent context when tracing is enabled.
    if state.tracing_enabled {
        use tracing_opentelemetry::OpenTelemetrySpanExt;
        span.set_parent(extract_otel_context(&parts.headers));
    }

    let mut result = check_inner(
        &state,
        &parts.headers,
        parts.method.as_str(),
        parts.uri.path(),
        &request_id,
    )
    .instrument(span.clone())
    .await;

    span.record("http.status_code", result.status().as_u16() as i64);
    span.record("duration_ms", start.elapsed().as_millis() as i64);

    // Inject traceparent into response so Envoy forwards it upstream.
    if state.tracing_enabled {
        inject_otel_context(&span, result.headers_mut());
    }

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

    tracing::Span::current().record("route", route.name.as_str());
    tracing::Span::current().record("upstream_service", route.upstream.service.as_str());

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
    let mut auth_subject = None;
    let plugin_continue = match execute_access_plugins(
        state,
        route,
        PluginExecutionMeta {
            headers,
            host,
            method,
            path,
            request_id,
            remote_addr: &remote_addr,
            start: &start,
        },
        &mut response_headers,
    )
    .await
    {
        Ok(result) => result,
        Err(response) => return response,
    };
    let plugin_request = plugin_continue.request;
    let plugin_upstream_headers = plugin_continue.upstream_headers;

    if let Err(deny) = apply_auth(
        state,
        cfg,
        route,
        headers,
        &mut response_headers,
        &mut auth_subject,
    )
    .await
    {
        run_plugin_log(state, &plugin_request, deny.status.as_u16()).await;
        return early_deny(
            state,
            request_id,
            &remote_addr,
            host,
            method,
            path,
            route,
            deny.status,
            &deny.error_code,
            &start,
        );
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
                run_plugin_log(state, &plugin_request, status.as_u16()).await;
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
            run_plugin_log(
                state,
                &plugin_request,
                StatusCode::SERVICE_UNAVAILABLE.as_u16(),
            )
            .await;
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

    insert_string_headers(&mut response_headers, &plugin_upstream_headers);
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
    run_plugin_log(state, &plugin_request, StatusCode::OK.as_u16()).await;
    allow_response(response_headers, request_id)
}
