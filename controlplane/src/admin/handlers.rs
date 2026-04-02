use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;

use super::responses::{
    ConfigStatusResponse, HealthResponse, ReadyResponse, ReloadErrorResponse, ReloadSuccessResponse,
};
use super::state::{now_unix, sha256_hex, ReloadResult, SharedState};
use crate::config;

/// `GET /healthz` -- liveness probe. Always returns 200.
pub(crate) async fn healthz() -> Json<HealthResponse> {
    Json(HealthResponse { ok: true })
}

/// `GET /readyz` -- readiness probe. Returns 200 if all dependencies healthy, 503 otherwise.
pub(crate) async fn readyz(State(state): State<SharedState>) -> impl IntoResponse {
    let cs = state.config_state.read().await;

    // Config is always loaded after startup (we exit on failure).
    let redis_ok = state.rate_limiter.ping().await;
    let jwks_ok = state.jwks_registry.all_healthy().await;
    let ok = redis_ok && jwks_ok;

    let status = if ok {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };

    let response = ReadyResponse {
        ok,
        config_loaded: true,
        redis_ok,
        jwks_ok,
        last_config_reload_unix: cs.last_reload_unix,
    };

    (status, Json(response))
}

/// `GET /config/status` -- current config metadata.
pub(crate) async fn config_status(State(state): State<SharedState>) -> Json<ConfigStatusResponse> {
    let cs = state.config_state.read().await;

    Json(ConfigStatusResponse {
        active_config_version: cs.config.version,
        active_config_sha256: cs.sha256.clone(),
        last_reload_result: cs.last_reload_result.as_str().to_owned(),
        last_reload_error: cs.last_reload_error.clone(),
        last_reload_unix: cs.last_reload_unix,
    })
}

/// `GET /metrics` -- Prometheus text exposition metrics.
pub(crate) async fn metrics(State(state): State<SharedState>) -> impl IntoResponse {
    match state.metrics.encode() {
        Ok(body) => (
            StatusCode::OK,
            [(
                axum::http::header::CONTENT_TYPE,
                "text/plain; version=0.0.4; charset=utf-8",
            )],
            body,
        )
            .into_response(),
        Err(e) => {
            tracing::error!(error = %e, "failed to encode metrics");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// `POST /config/reload` -- reload config from disk.
pub(crate) async fn config_reload(State(state): State<SharedState>) -> impl IntoResponse {
    let now = now_unix();

    // Read raw bytes from disk.
    let raw = match std::fs::read(&state.config_path) {
        Ok(bytes) => bytes,
        Err(e) => {
            let mut cs = state.config_state.write().await;
            let error_msg = format!("failed to read {}: {e}", state.config_path.display());
            cs.last_reload_unix = now;
            cs.last_reload_result = ReloadResult::ValidationError;
            cs.last_reload_error = Some(error_msg.clone());
            state.metrics.record_config_reload("validation_error");

            return (
                StatusCode::BAD_REQUEST,
                Json(
                    serde_json::to_value(ReloadErrorResponse {
                        ok: false,
                        message: "config validation failed".into(),
                        error: error_msg,
                        reload_timestamp: now,
                    })
                    .expect("serialization cannot fail"),
                ),
            );
        }
    };

    let yaml = String::from_utf8_lossy(&raw);

    // Parse and validate.
    match config::load_config_from_str(&yaml) {
        Ok(new_config) => {
            let new_sha = sha256_hex(&raw);
            let mut cs = state.config_state.write().await;
            let previous_sha = cs.sha256.clone();

            cs.config = new_config;
            cs.sha256 = new_sha.clone();
            cs.last_reload_unix = now;
            cs.last_reload_result = ReloadResult::Success;
            cs.last_reload_error = None;
            state.metrics.record_config_reload("success");

            // Regenerate and write Envoy config if path is configured.
            if let Some(ref envoy_path) = state.envoy_config_path {
                match config::generate_envoy_config(&cs.config) {
                    Ok(envoy_yaml) => {
                        if let Err(e) = std::fs::write(envoy_path, &envoy_yaml) {
                            tracing::warn!(
                                path = %envoy_path.display(),
                                error = %e,
                                "failed to write envoy config"
                            );
                        } else {
                            tracing::info!(
                                path = %envoy_path.display(),
                                "envoy config regenerated"
                            );
                        }
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "envoy config generation failed on reload");
                    }
                }
            }

            tracing::info!(
                previous_sha256 = %previous_sha,
                new_sha256 = %new_sha,
                "config reloaded"
            );

            (
                StatusCode::OK,
                Json(
                    serde_json::to_value(ReloadSuccessResponse {
                        ok: true,
                        message: "config reloaded successfully".into(),
                        previous_sha256: previous_sha,
                        new_sha256: new_sha,
                        reload_timestamp: now,
                    })
                    .expect("serialization cannot fail"),
                ),
            )
        }
        Err(errs) => {
            let error_msg = errs.to_string();
            let mut cs = state.config_state.write().await;
            cs.last_reload_unix = now;
            cs.last_reload_result = ReloadResult::ValidationError;
            cs.last_reload_error = Some(error_msg.clone());
            state.metrics.record_config_reload("validation_error");

            tracing::warn!(%errs, "config reload validation failed");

            (
                StatusCode::BAD_REQUEST,
                Json(
                    serde_json::to_value(ReloadErrorResponse {
                        ok: false,
                        message: "config validation failed".into(),
                        error: error_msg,
                        reload_timestamp: now,
                    })
                    .expect("serialization cannot fail"),
                ),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use axum::body::Body;
    use axum::http::{Method, Request};
    use axum::Router;
    use tower::ServiceExt;

    use super::super::state::build_state;
    use super::*;

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
        );

        Router::new()
            .route("/healthz", axum::routing::get(healthz))
            .route("/readyz", axum::routing::get(readyz))
            .route("/config/status", axum::routing::get(config_status))
            .route("/config/reload", axum::routing::post(config_reload))
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
        // test_router uses offline_for_test (no Redis), so redis_ok=false → 503.
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
        // IntGauge always present; CounterVec only after observation.
        assert!(text.contains("gateway_inflight_requests"));
    }
}
