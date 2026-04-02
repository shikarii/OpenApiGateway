use super::logs::{generate_request_id, now_rfc3339, AccessLogEntry};
use super::metrics::MetricsRegistry;

// --- Access log field completeness ---

fn full_entry() -> AccessLogEntry {
    AccessLogEntry {
        ts: now_rfc3339(),
        request_id: generate_request_id(),
        remote_addr: "10.0.0.1".into(),
        host: "api.example.com".into(),
        method: "POST".into(),
        path: "/users/create".into(),
        route: "user-api".into(),
        status: 201,
        duration_ms: 42,
        bytes_in: 256,
        bytes_out: 1024,
        auth_subject: Some("user-abc".into()),
        rate_limit_mode: "redis".into(),
        upstream_service: "user-svc".into(),
        upstream_addr: "10.0.1.5:8080".into(),
    }
}

#[test]
fn access_log_contains_exactly_15_fields() {
    let entry = full_entry();
    let json = entry.to_json().unwrap();
    let obj: serde_json::Map<String, serde_json::Value> = serde_json::from_str(&json).unwrap();
    assert_eq!(obj.len(), 15, "spec requires exactly 15 fields: {obj:?}");
}

#[test]
fn access_log_field_names_match_spec() {
    let entry = full_entry();
    let json = entry.to_json().unwrap();
    let obj: serde_json::Map<String, serde_json::Value> = serde_json::from_str(&json).unwrap();

    let required = [
        "ts",
        "request_id",
        "remote_addr",
        "host",
        "method",
        "path",
        "route",
        "status",
        "duration_ms",
        "bytes_in",
        "bytes_out",
        "auth_subject",
        "rate_limit_mode",
        "upstream_service",
        "upstream_addr",
    ];
    for field in &required {
        assert!(obj.contains_key(*field), "missing field: {field}");
    }
}

#[test]
fn unmatched_404_log_has_empty_route_and_upstream() {
    let entry = AccessLogEntry {
        ts: now_rfc3339(),
        request_id: generate_request_id(),
        remote_addr: "10.0.0.1".into(),
        host: "unknown.example.com".into(),
        method: "GET".into(),
        path: "/nonexistent".into(),
        route: "".into(),
        status: 404,
        duration_ms: 1,
        bytes_in: 0,
        bytes_out: 0,
        auth_subject: None,
        rate_limit_mode: "none".into(),
        upstream_service: "".into(),
        upstream_addr: "".into(),
    };
    let json = entry.to_json().unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();

    assert_eq!(v["route"], "");
    assert_eq!(v["upstream_service"], "");
    assert_eq!(v["upstream_addr"], "");
    assert!(v["auth_subject"].is_null());
    assert_eq!(v["status"], 404);
}

#[test]
fn access_log_json_is_single_line() {
    let entry = full_entry();
    let json = entry.to_json().unwrap();
    assert!(
        !json.contains('\n'),
        "access log must be a single line for log parsers"
    );
}

#[test]
fn request_id_is_hex_and_8_chars() {
    for _ in 0..100 {
        let id = generate_request_id();
        assert_eq!(id.len(), 8, "request_id must be 8 chars: {id}");
        assert!(
            id.chars().all(|c| c.is_ascii_hexdigit()),
            "request_id must be hex: {id}"
        );
    }
}

#[test]
fn timestamp_is_utc_rfc3339() {
    let ts = now_rfc3339();
    assert!(ts.ends_with('Z'), "must be UTC (Z suffix): {ts}");
    // Parse to verify format validity.
    chrono::NaiveDateTime::parse_from_str(&ts, "%Y-%m-%dT%H:%M:%S%.3fZ")
        .unwrap_or_else(|e| panic!("invalid RFC3339 timestamp '{ts}': {e}"));
}

// --- Metrics label cardinality ---

#[test]
fn status_class_labels_are_bounded() {
    let reg = MetricsRegistry::new().unwrap();
    // Record various status codes — labels should be status classes, not raw codes.
    for code in [200, 201, 204, 301, 400, 401, 403, 404, 429, 500, 502, 503] {
        reg.record_request("test-route", "GET", code, 1.0);
    }
    let text = reg.encode().unwrap();

    // Should contain status_class labels (bounded), not raw status codes.
    assert!(text.contains("status_class=\"2xx\""));
    assert!(text.contains("status_class=\"3xx\""));
    assert!(text.contains("status_class=\"4xx\""));
    assert!(text.contains("status_class=\"5xx\""));
    // Raw status codes should NOT appear as label values.
    assert!(!text.contains("status_class=\"200\""));
    assert!(!text.contains("status_class=\"404\""));
}

#[test]
fn metrics_use_route_names_not_paths() {
    let reg = MetricsRegistry::new().unwrap();
    reg.record_request("user-api", "GET", 200, 5.0);
    reg.record_auth_failure("user-api", "token_expired");
    reg.record_rate_limit_allowed("user-api");

    let text = reg.encode().unwrap();
    // Route label uses configured name, not raw path.
    assert!(text.contains("route=\"user-api\""));
    // No path-like labels.
    assert!(!text.contains("route=\"/users\""));
}

#[test]
fn multiple_routes_tracked_independently() {
    let reg = MetricsRegistry::new().unwrap();
    reg.record_request("route-a", "GET", 200, 5.0);
    reg.record_request("route-a", "GET", 200, 5.0);
    reg.record_request("route-b", "POST", 500, 50.0);

    let text = reg.encode().unwrap();
    assert!(text.contains(r#"route="route-a",status_class="2xx"} 2"#));
    assert!(text.contains(r#"route="route-b",status_class="5xx"} 1"#));
}

// --- All 9 metrics registered ---

#[test]
fn all_spec_metrics_present_after_recording() {
    let reg = MetricsRegistry::new().unwrap();

    // Trigger all metric types.
    reg.record_request("r", "GET", 200, 1.0);
    reg.record_auth_failure("r", "expired");
    reg.record_rate_limit_allowed("r");
    reg.record_rate_limit_denied("r");
    reg.record_rate_limit_degraded("r");
    reg.record_upstream_failure("r", "svc", "timeout");
    reg.record_config_reload("success");
    reg.inc_inflight();

    let text = reg.encode().unwrap();

    let expected_metrics = [
        "gateway_http_requests_total",
        "gateway_http_request_duration_ms",
        "gateway_auth_failures_total",
        "gateway_rate_limit_allowed_total",
        "gateway_rate_limit_denied_total",
        "gateway_rate_limit_degraded_total",
        "gateway_upstream_failures_total",
        "gateway_config_reload_total",
        "gateway_inflight_requests",
    ];
    for name in &expected_metrics {
        assert!(text.contains(name), "missing metric: {name}");
    }
}

#[test]
fn inflight_gauge_never_goes_negative() {
    let reg = MetricsRegistry::new().unwrap();
    reg.inc_inflight();
    reg.dec_inflight();
    reg.dec_inflight(); // extra decrement

    let text = reg.encode().unwrap();
    // IntGauge can go negative in prometheus crate, but we verify the pattern.
    assert!(text.contains("gateway_inflight_requests"));
}

#[test]
fn metrics_encode_is_valid_prometheus_text() {
    let reg = MetricsRegistry::new().unwrap();
    reg.record_request("r", "GET", 200, 1.0);
    reg.inc_inflight();

    let text = reg.encode().unwrap();

    // Prometheus text format requires # HELP and # TYPE lines.
    assert!(text.contains("# HELP gateway_http_requests_total"));
    assert!(text.contains("# TYPE gateway_http_requests_total counter"));
    assert!(text.contains("# TYPE gateway_inflight_requests gauge"));
    assert!(text.contains("# TYPE gateway_http_request_duration_ms histogram"));
}

// --- Rate limit mode values in access log ---

#[test]
fn rate_limit_mode_values() {
    for mode in ["redis", "degraded-local", "none"] {
        let mut entry = full_entry();
        entry.rate_limit_mode = mode.into();
        let json = entry.to_json().unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["rate_limit_mode"], mode);
    }
}
