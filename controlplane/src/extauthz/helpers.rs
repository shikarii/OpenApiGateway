use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use std::collections::HashMap;

use shared::config_types::RouteConfig;

use crate::admin::state::SharedState;
use crate::observability::{now_rfc3339, AccessLogEntry};

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

/// Result of parsing the Authorization header for a Bearer token.
#[derive(Debug)]
pub(super) enum BearerResult<'a> {
    /// Valid Bearer token extracted.
    Valid(&'a str),
    /// Authorization header present with Bearer scheme but empty/malformed.
    Malformed,
}

/// Extract the Bearer token from the Authorization header.
///
/// Returns `None` if no Authorization header, `Malformed` if the header
/// uses Bearer scheme but the token is empty, `Valid` otherwise.
pub(super) fn extract_bearer(headers: &HeaderMap) -> Option<BearerResult<'_>> {
    let value = headers.get("authorization")?.to_str().ok()?;
    let token = value.strip_prefix("Bearer ")?;
    if token.is_empty() {
        return Some(BearerResult::Malformed);
    }
    Some(BearerResult::Valid(token))
}

pub(super) fn client_ip(headers: &HeaderMap, trust_forwarded: bool) -> String {
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

pub(super) fn header_str<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name)?.to_str().ok()
}

/// Insert rate-limit response headers from a rate decision.
pub(super) fn insert_rate_limit_headers(
    headers: &mut HeaderMap,
    decision: &crate::ratelimit::RateDecision,
) {
    if let Ok(v) = decision.remaining.to_string().parse() {
        headers.insert("x-rate-limit-remaining", v);
    }
    if let Ok(v) = decision.limit.to_string().parse() {
        headers.insert("x-rate-limit-limit", v);
    }
    if let Ok(v) = decision.mode.as_header_value().parse() {
        headers.insert("x-rate-limit-mode", v);
    }
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let reset = now_secs + (decision.retry_after_ms / 1000).max(1);
    if let Ok(v) = reset.to_string().parse() {
        headers.insert("x-rate-limit-reset", v);
    }
}

pub(super) fn insert_string_headers(headers: &mut HeaderMap, values: &HashMap<String, String>) {
    for (name, value) in values {
        if let (Ok(header_name), Ok(header_value)) = (
            name.parse::<axum::http::HeaderName>(),
            value.parse::<axum::http::HeaderValue>(),
        ) {
            headers.insert(header_name, header_value);
        }
    }
}

pub(super) fn overload_response(request_id: &str) -> axum::response::Response {
    let mut headers = HeaderMap::new();
    if let Ok(v) = "true".parse() {
        headers.insert("x-gateway-overloaded", v);
    }
    if let Ok(v) = "1".parse() {
        headers.insert("retry-after", v);
    }
    if let Ok(v) = request_id.parse() {
        headers.insert("x-request-id", v);
    }
    let body = serde_json::json!({"error": "gateway_overloaded"});
    (StatusCode::SERVICE_UNAVAILABLE, headers, axum::Json(body)).into_response()
}

pub(super) fn allow_response(mut headers: HeaderMap, request_id: &str) -> axum::response::Response {
    if let Ok(v) = request_id.parse() {
        headers.insert("x-request-id", v);
    }
    (StatusCode::OK, headers).into_response()
}

pub(super) fn deny_response(
    status: StatusCode,
    error_code: &str,
    request_id: &str,
) -> axum::response::Response {
    let mut headers = HeaderMap::new();
    if let Ok(v) = request_id.parse() {
        headers.insert("x-request-id", v);
    }
    let body = serde_json::json!({"error": error_code});
    (status, headers, axum::Json(body)).into_response()
}

/// Helper for method-not-allowed: emits log + metric, returns 405.
#[allow(clippy::too_many_arguments)]
pub(super) fn method_denied(
    state: &SharedState,
    request_id: &str,
    remote_addr: &str,
    host: &str,
    method: &str,
    path: &str,
    route: &RouteConfig,
    start: &std::time::Instant,
) -> axum::response::Response {
    early_deny(
        state,
        request_id,
        remote_addr,
        host,
        method,
        path,
        route,
        StatusCode::METHOD_NOT_ALLOWED,
        "method_not_allowed",
        start,
    )
}

/// Helper for early deny paths: emits access log + request metric, returns deny response.
#[allow(clippy::too_many_arguments)]
pub(super) fn early_deny(
    state: &SharedState,
    request_id: &str,
    remote_addr: &str,
    host: &str,
    method: &str,
    path: &str,
    route: &RouteConfig,
    status: StatusCode,
    error_code: &str,
    start: &std::time::Instant,
) -> axum::response::Response {
    let code = status.as_u16();
    emit_log(
        request_id,
        remote_addr,
        host,
        method,
        path,
        &route.name,
        code,
        start,
        None,
        "none",
        &route.upstream.service,
    );
    state.metrics.record_request(
        &route.name,
        method,
        code,
        start.elapsed().as_millis() as f64,
    );
    deny_response(status, error_code, request_id)
}

/// Emit a JSON access log entry to stdout.
#[allow(clippy::too_many_arguments)]
pub(super) fn emit_log(
    request_id: &str,
    remote_addr: &str,
    host: &str,
    method: &str,
    path: &str,
    route: &str,
    status: u16,
    start: &std::time::Instant,
    auth_subject: Option<&str>,
    rate_limit_mode: &str,
    upstream_service: &str,
) {
    let entry = AccessLogEntry {
        ts: now_rfc3339(),
        request_id: request_id.to_owned(),
        remote_addr: remote_addr.to_owned(),
        host: host.to_owned(),
        method: method.to_owned(),
        path: path.to_owned(),
        route: route.to_owned(),
        status,
        duration_ms: start.elapsed().as_millis() as u64,
        bytes_in: 0,
        bytes_out: 0,
        auth_subject: auth_subject.map(|s| s.to_owned()),
        rate_limit_mode: rate_limit_mode.to_owned(),
        upstream_service: upstream_service.to_owned(),
        upstream_addr: String::new(),
    };
    entry.emit();
}

// ---------------------------------------------------------------------------
// OpenTelemetry trace-context propagation helpers
// ---------------------------------------------------------------------------

/// Adapter for extracting W3C traceparent from [`HeaderMap`].
pub(super) struct HeaderExtractor<'a>(pub &'a HeaderMap);

impl opentelemetry::propagation::Extractor for HeaderExtractor<'_> {
    fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).and_then(|v| v.to_str().ok())
    }

    fn keys(&self) -> Vec<&str> {
        self.0.keys().map(|k| k.as_str()).collect()
    }
}

/// Adapter for injecting W3C traceparent into [`HeaderMap`].
pub(super) struct HeaderInjector<'a>(pub &'a mut HeaderMap);

impl opentelemetry::propagation::Injector for HeaderInjector<'_> {
    fn set(&mut self, key: &str, value: String) {
        if let (Ok(name), Ok(val)) = (
            key.parse::<axum::http::HeaderName>(),
            value.parse::<axum::http::HeaderValue>(),
        ) {
            self.0.insert(name, val);
        }
    }
}

/// Extract an OpenTelemetry context from inbound request headers.
pub(super) fn extract_otel_context(headers: &HeaderMap) -> opentelemetry::Context {
    opentelemetry::global::get_text_map_propagator(|propagator| {
        propagator.extract(&HeaderExtractor(headers))
    })
}

/// Inject the current span's trace context into response headers.
pub(super) fn inject_otel_context(span: &tracing::Span, headers: &mut HeaderMap) {
    use tracing_opentelemetry::OpenTelemetrySpanExt;
    let cx = span.context();
    opentelemetry::global::get_text_map_propagator(|propagator| {
        propagator.inject_context(&cx, &mut HeaderInjector(headers));
    });
}
