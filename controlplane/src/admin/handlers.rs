use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;

use super::responses::{
    ConfigStatusResponse, HealthResponse, ReadyResponse, ReloadErrorResponse,
    ReloadSuccessResponse, XdsStatusResponse,
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
    let redis_ok = state.rate_limiter.ping().await;
    let jwks_ok = state.jwks_registry.all_healthy().await;
    let ok = redis_ok && jwks_ok;
    let status = if ok {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };

    (
        status,
        Json(ReadyResponse {
            ok,
            config_loaded: true,
            redis_ok,
            jwks_ok,
            last_config_reload_unix: cs.last_reload_unix,
        }),
    )
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
        Err(error) => {
            tracing::error!(error = %error, "failed to encode metrics");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// `GET /xds/status` -- current ADS peer state.
pub(crate) async fn xds_status(State(state): State<SharedState>) -> Json<XdsStatusResponse> {
    match state.xds.as_ref() {
        Some(xds) => Json(XdsStatusResponse {
            enabled: true,
            connected_envoys: xds.connected_envoys() as usize,
            peers: xds.statuses().await,
        }),
        None => Json(XdsStatusResponse {
            enabled: false,
            connected_envoys: 0,
            peers: Vec::new(),
        }),
    }
}

/// `POST /config/reload` -- reload config from disk.
pub(crate) async fn config_reload(State(state): State<SharedState>) -> impl IntoResponse {
    let now = now_unix();
    let raw = match std::fs::read(&state.config_path) {
        Ok(bytes) => bytes,
        Err(error) => {
            return reload_validation_error(
                &state,
                now,
                format!("failed to read {}: {error}", state.config_path.display()),
            )
            .await
        }
    };
    let yaml = String::from_utf8_lossy(&raw);

    match config::load_config_from_str(&yaml) {
        Ok(new_config) => apply_config_reload(state, raw, now, new_config).await,
        Err(errors) => reload_validation_error(&state, now, errors.to_string()).await,
    }
}

async fn apply_config_reload(
    state: SharedState,
    raw: Vec<u8>,
    now: i64,
    new_config: shared::config_types::GatewayConfig,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Some(plugin_engine) = state.plugin_engine.as_ref() {
        if let Err(error) = plugin_engine.reload_from_config(&new_config).await {
            return reload_validation_error(&state, now, error.to_string()).await;
        }
    }

    if let Some(xds) = state.xds.as_ref() {
        if let Err(error) = xds.rebuild_from_config(&new_config).await {
            return reload_validation_error(&state, now, error.to_string()).await;
        }
    }

    let new_sha = sha256_hex(&raw);
    let mut cs = state.config_state.write().await;
    let previous_sha = cs.sha256.clone();
    cs.config = new_config;
    cs.sha256 = new_sha.clone();
    cs.last_reload_unix = now;
    cs.last_reload_result = ReloadResult::Success;
    cs.last_reload_error = None;
    state.metrics.record_config_reload("success");

    if let Some(ref envoy_path) = state.envoy_config_path {
        match config::generate_envoy_config(&cs.config) {
            Ok(envoy_yaml) => {
                if let Err(error) = std::fs::write(envoy_path, &envoy_yaml) {
                    tracing::warn!(
                        path = %envoy_path.display(),
                        error = %error,
                        "failed to write envoy config"
                    );
                } else {
                    tracing::info!(path = %envoy_path.display(), "envoy config regenerated");
                }
            }
            Err(error) => {
                tracing::warn!(error = %error, "envoy config generation failed on reload")
            }
        }
    }

    tracing::info!(previous_sha256 = %previous_sha, new_sha256 = %new_sha, "config reloaded");
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

async fn reload_validation_error(
    state: &SharedState,
    now: i64,
    error_msg: String,
) -> (StatusCode, Json<serde_json::Value>) {
    let mut cs = state.config_state.write().await;
    cs.last_reload_unix = now;
    cs.last_reload_result = ReloadResult::ValidationError;
    cs.last_reload_error = Some(error_msg.clone());
    state.metrics.record_config_reload("validation_error");
    tracing::warn!(error = %error_msg, "config reload validation failed");

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
