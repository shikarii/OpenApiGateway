use super::*;

#[test]
fn registry_creates_successfully() {
    let reg = MetricsRegistry::new().unwrap();
    let text = reg.encode().unwrap();
    // IntGauge (no labels) always appears even without observations.
    assert!(text.contains("gateway_inflight_requests"));
    // CounterVec only appears after first observation.
    reg.record_request("test", "GET", 200, 1.0);
    let text = reg.encode().unwrap();
    assert!(text.contains("gateway_http_requests_total"));
}

#[test]
fn status_class_mapping() {
    assert_eq!(status_class(100), "1xx");
    assert_eq!(status_class(199), "1xx");
    assert_eq!(status_class(200), "2xx");
    assert_eq!(status_class(204), "2xx");
    assert_eq!(status_class(301), "3xx");
    assert_eq!(status_class(400), "4xx");
    assert_eq!(status_class(404), "4xx");
    assert_eq!(status_class(500), "5xx");
    assert_eq!(status_class(503), "5xx");
    assert_eq!(status_class(599), "5xx");
}

#[test]
fn record_request_increments_counter() {
    let reg = MetricsRegistry::new().unwrap();
    reg.record_request("my-route", "GET", 200, 42.0);
    reg.record_request("my-route", "GET", 200, 10.0);

    let text = reg.encode().unwrap();
    assert!(text.contains(
        r#"gateway_http_requests_total{method="GET",route="my-route",status_class="2xx"} 2"#
    ));
}

#[test]
fn histogram_records_duration() {
    let reg = MetricsRegistry::new().unwrap();
    reg.record_request("api", "POST", 201, 15.0);

    let text = reg.encode().unwrap();
    assert!(text.contains("gateway_http_request_duration_ms_bucket"));
    assert!(text.contains("gateway_http_request_duration_ms_sum"));
    assert!(text.contains("gateway_http_request_duration_ms_count"));
}

#[test]
fn config_reload_counter() {
    let reg = MetricsRegistry::new().unwrap();
    reg.record_config_reload("success");
    reg.record_config_reload("success");
    reg.record_config_reload("validation_error");

    let text = reg.encode().unwrap();
    assert!(text.contains(r#"gateway_config_reload_total{result="success"} 2"#));
    assert!(text.contains(r#"gateway_config_reload_total{result="validation_error"} 1"#));
}

#[test]
fn inflight_gauge_increment_decrement() {
    let reg = MetricsRegistry::new().unwrap();
    reg.inc_inflight();
    reg.inc_inflight();
    reg.inc_inflight();
    reg.dec_inflight();

    let text = reg.encode().unwrap();
    assert!(text.contains("gateway_inflight_requests 2"));
}

#[test]
fn auth_failure_records() {
    let reg = MetricsRegistry::new().unwrap();
    reg.record_auth_failure("private-api", "token_expired");
    reg.record_auth_failure("private-api", "invalid_signature");
    reg.record_auth_failure("private-api", "token_expired");

    let text = reg.encode().unwrap();
    assert!(text
        .contains(r#"gateway_auth_failures_total{reason="token_expired",route="private-api"} 2"#));
    assert!(text.contains(
        r#"gateway_auth_failures_total{reason="invalid_signature",route="private-api"} 1"#
    ));
}

#[test]
fn rate_limit_counters() {
    let reg = MetricsRegistry::new().unwrap();
    reg.record_rate_limit_allowed("public-api");
    reg.record_rate_limit_denied("public-api");
    reg.record_rate_limit_degraded("public-api");

    let text = reg.encode().unwrap();
    assert!(text.contains(r#"gateway_rate_limit_allowed_total{route="public-api"} 1"#));
    assert!(text.contains(r#"gateway_rate_limit_denied_total{route="public-api"} 1"#));
    assert!(text.contains(r#"gateway_rate_limit_degraded_total{route="public-api"} 1"#));
}

#[test]
fn upstream_failure_records() {
    let reg = MetricsRegistry::new().unwrap();
    reg.record_upstream_failure("api", "backend", "connection_timeout");

    let text = reg.encode().unwrap();
    assert!(text.contains(
        r#"gateway_upstream_failures_total{reason="connection_timeout",route="api",service="backend"} 1"#
    ));
}

#[test]
fn encode_produces_valid_text() {
    let reg = MetricsRegistry::new().unwrap();
    reg.record_request("r1", "GET", 200, 5.0);

    let text = reg.encode().unwrap();
    // Prometheus text format starts with # HELP or # TYPE lines.
    assert!(text.contains("# HELP"));
    assert!(text.contains("# TYPE"));
    // Every line should be valid UTF-8 (already guaranteed by String).
    assert!(!text.is_empty());
}
