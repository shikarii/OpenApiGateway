use std::path::PathBuf;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{HeaderValue, Request, StatusCode};
use axum::Router;
use shared::config_types::RouteConfig;
use tokio::sync::Semaphore;
use tower::ServiceExt;

use super::handler::router;
use super::helpers::{extract_bearer, match_route, BearerResult};
use crate::admin::state::build_state;
use crate::config;

// --- Route matching unit tests ---

fn make_route(name: &str, hosts: &[&str], prefix: &str) -> RouteConfig {
    RouteConfig {
        name: name.into(),
        hostnames: hosts.iter().map(|s| s.to_string()).collect(),
        path_prefix: prefix.into(),
        methods: vec!["GET".into()],
        auth_required: false,
        auth_provider: None,
        required_scopes: None,
        rate_limit: shared::config_types::RouteRateLimit {
            bucket_capacity: 100,
            refill_rate_per_sec: 10.0,
            key_by: "ip".into(),
        },
        upstream: shared::config_types::UpstreamConfig {
            service: "backend".into(),
            request_timeout_ms: 5000,
            retries: 0,
        },
    }
}

#[test]
fn match_route_exact_hostname() {
    let routes = vec![
        make_route("api", &["api.example.com"], "/v1"),
        make_route("admin", &["admin.example.com"], "/v1"),
    ];
    let matched = match_route(&routes, "api.example.com", "/v1/users");
    assert_eq!(matched.unwrap().name, "api");
}

#[test]
fn match_route_wildcard_hostname() {
    let routes = vec![make_route("catch-all", &["*"], "/")];
    let matched = match_route(&routes, "any.host.com", "/some/path");
    assert_eq!(matched.unwrap().name, "catch-all");
}

#[test]
fn match_route_longest_prefix_wins() {
    let routes = vec![
        make_route("short", &["*"], "/api"),
        make_route("long", &["*"], "/api/v2"),
    ];
    let matched = match_route(&routes, "example.com", "/api/v2/users");
    assert_eq!(matched.unwrap().name, "long");
}

#[test]
fn match_route_no_match_returns_none() {
    let routes = vec![make_route("api", &["api.example.com"], "/v1")];
    assert!(match_route(&routes, "other.com", "/v1/users").is_none());
    assert!(match_route(&routes, "api.example.com", "/v2/users").is_none());
}

#[test]
fn match_route_exact_host_and_wildcard_both_match() {
    let routes = vec![
        make_route("specific", &["api.example.com"], "/v1"),
        make_route("wildcard", &["*"], "/v1"),
    ];
    let matched = match_route(&routes, "api.example.com", "/v1/x");
    assert!(matched.is_some());
}

// --- Bearer token extraction tests ---

#[test]
fn extract_bearer_valid() {
    let mut headers = axum::http::HeaderMap::new();
    headers.insert("authorization", HeaderValue::from_static("Bearer abc123"));
    match extract_bearer(&headers) {
        Some(BearerResult::Valid(t)) => assert_eq!(t, "abc123"),
        other => panic!("expected Valid, got {other:?}"),
    }
}

#[test]
fn extract_bearer_missing() {
    let headers = axum::http::HeaderMap::new();
    assert!(extract_bearer(&headers).is_none());
}

#[test]
fn extract_bearer_wrong_scheme() {
    let mut headers = axum::http::HeaderMap::new();
    headers.insert("authorization", HeaderValue::from_static("Basic dXNlcjpw"));
    assert!(extract_bearer(&headers).is_none());
}

#[test]
fn extract_bearer_empty_token() {
    let mut headers = axum::http::HeaderMap::new();
    headers.insert("authorization", HeaderValue::from_static("Bearer "));
    assert!(matches!(
        extract_bearer(&headers),
        Some(BearerResult::Malformed)
    ));
}

// --- Integration tests ---

fn test_router() -> Router {
    let yaml = include_str!("../../../examples/configs/gateway-single-node.yaml");
    let cfg = config::load_config_from_str(yaml).unwrap();
    let jwks_registry = crate::auth::JwksCacheRegistry::empty_for_test();
    let rate_limiter = crate::ratelimit::RateLimiter::offline_for_test(cfg.rate_limits.clone());
    let metrics = Arc::new(crate::observability::MetricsRegistry::new().unwrap());
    let state = build_state(
        cfg,
        yaml.as_bytes(),
        PathBuf::from("nonexistent.yaml"),
        None,
        jwks_registry,
        rate_limiter,
        metrics,
    );
    router(state)
}

#[tokio::test]
async fn check_unmatched_route_returns_200() {
    let app = test_router();
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/nonexistent/path")
                .header("host", "unknown.host.com")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    assert!(resp.headers().get("x-request-id").is_some());
}

#[tokio::test]
async fn check_matched_route_allows_or_rate_limits() {
    let app = test_router();
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/public/something")
                .header("host", "example.com")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // Rate limiter is offline, so either fail-open (200) or unavailable (503).
    let status = resp.status();
    assert!(
        status == StatusCode::OK
            || status == StatusCode::SERVICE_UNAVAILABLE
            || status == StatusCode::TOO_MANY_REQUESTS,
        "unexpected status: {status}"
    );
    assert!(resp.headers().get("x-request-id").is_some());
}

#[tokio::test]
async fn check_method_not_allowed_returns_405() {
    let app = test_router();
    // gateway-single-node.yaml routes allow GET only; send DELETE.
    let resp = app
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri("/public/something")
                .header("host", "example.com")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert!(resp.headers().get("x-request-id").is_some());
}

#[tokio::test]
async fn check_overloaded_returns_503() {
    let yaml = include_str!("../../../examples/configs/gateway-single-node.yaml");
    let cfg = config::load_config_from_str(yaml).unwrap();
    let jwks_registry = crate::auth::JwksCacheRegistry::empty_for_test();
    let rate_limiter = crate::ratelimit::RateLimiter::offline_for_test(cfg.rate_limits.clone());
    let metrics = Arc::new(crate::observability::MetricsRegistry::new().unwrap());

    let state = build_state(
        cfg,
        yaml.as_bytes(),
        PathBuf::from("nonexistent.yaml"),
        None,
        jwks_registry,
        rate_limiter,
        metrics,
    );
    // Reconstruct with zero permits to simulate overload.
    let overloaded_state = Arc::new(crate::admin::state::AppState {
        config_state: tokio::sync::RwLock::new(state.config_state.read().await.clone()),
        config_path: state.config_path.clone(),
        envoy_config_path: None,
        jwks_registry: Arc::clone(&state.jwks_registry),
        rate_limiter: Arc::clone(&state.rate_limiter),
        metrics: Arc::clone(&state.metrics),
        concurrency_limit: Arc::new(Semaphore::new(0)),
        tracing_enabled: false,
    });

    let app = router(overloaded_state);
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/public/test")
                .header("host", "example.com")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(resp.headers().get("x-gateway-overloaded").unwrap(), "true");
    assert_eq!(resp.headers().get("retry-after").unwrap(), "1");
    assert!(resp.headers().get("x-request-id").is_some());
}
