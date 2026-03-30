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

/// `GET /readyz` -- readiness probe. Returns 200 if config is loaded.
pub(crate) async fn readyz(State(state): State<SharedState>) -> impl IntoResponse {
    let cs = state.config_state.read().await;

    // Config is always loaded after startup (we exit on failure).
    let jwks_ok = state.jwks_registry.all_healthy().await;
    let response = ReadyResponse {
        ok: true,
        config_loaded: true,
        redis_ok: true, // TODO(#12): real Redis check
        jwks_ok,
        last_config_reload_unix: cs.last_reload_unix,
    };

    (StatusCode::OK, Json(response))
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
        let state = build_state(
            cfg,
            yaml.as_bytes(),
            PathBuf::from("nonexistent.yaml"),
            jwks_registry,
        );

        Router::new()
            .route("/healthz", axum::routing::get(healthz))
            .route("/readyz", axum::routing::get(readyz))
            .route("/config/status", axum::routing::get(config_status))
            .route("/config/reload", axum::routing::post(config_reload))
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
    async fn readyz_returns_200() {
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

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["ok"], true);
        assert_eq!(json["config_loaded"], true);
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
}
