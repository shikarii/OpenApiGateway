use std::path::PathBuf;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use axum::Router;
use tower::ServiceExt;

use super::handlers::{config_reload, config_status, healthz, metrics, readyz, xds_status};
use super::state::build_state;
use crate::config;

fn test_router() -> Router {
    let yaml = include_str!("../../../examples/configs/gateway-single-node.yaml");
    let cfg = config::load_config_from_str(yaml).unwrap();
    let jwks_registry = crate::auth::JwksCacheRegistry::empty_for_test();
    let rate_limiter = crate::ratelimit::RateLimiter::offline_for_test(cfg.rate_limits.clone());
    let metrics_registry =
        std::sync::Arc::new(crate::observability::MetricsRegistry::new().unwrap());
    let state = build_state(
        cfg,
        yaml.as_bytes(),
        PathBuf::from("nonexistent.yaml"),
        None,
        jwks_registry,
        rate_limiter,
        metrics_registry,
        None,
        None,
    );

    Router::new()
        .route("/healthz", axum::routing::get(healthz))
        .route("/readyz", axum::routing::get(readyz))
        .route("/config/status", axum::routing::get(config_status))
        .route("/config/reload", axum::routing::post(config_reload))
        .route("/xds/status", axum::routing::get(xds_status))
        .route("/metrics", axum::routing::get(metrics))
        .with_state(state)
}

#[tokio::test]
async fn healthz_returns_200() {
    let app = test_router();
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/healthz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["ok"], true);
}

#[tokio::test]
async fn readyz_returns_503_when_redis_offline() {
    let app = test_router();
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/readyz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["ok"], false);
    assert_eq!(json["config_loaded"], true);
    assert_eq!(json["redis_ok"], false);
    assert_eq!(json["jwks_ok"], true);
}

#[tokio::test]
async fn config_status_returns_version_and_hash() {
    let app = test_router();
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/config/status")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["active_config_version"], 1);
    assert!(json["active_config_sha256"].as_str().unwrap().len() == 64);
    assert_eq!(json["last_reload_result"], "success");
    assert!(json["last_reload_error"].is_null());
}

#[tokio::test]
async fn config_reload_returns_400_when_file_missing() {
    let app = test_router();
    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/config/reload")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["ok"], false);
}

#[tokio::test]
async fn metrics_returns_prometheus_text() {
    let app = test_router();
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/metrics")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let ct = resp
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(ct.contains("text/plain"));

    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let text = String::from_utf8(body.to_vec()).unwrap();
    assert!(text.contains("gateway_inflight_requests"));
}

#[tokio::test]
async fn xds_status_reports_disabled_when_not_configured() {
    let app = test_router();
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/xds/status")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["enabled"], false);
    assert_eq!(json["connected_envoys"], 0);
}
