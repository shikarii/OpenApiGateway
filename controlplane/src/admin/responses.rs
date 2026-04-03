use serde::Serialize;

/// Response for `GET /healthz`.
#[derive(Debug, Serialize)]
pub(crate) struct HealthResponse {
    pub ok: bool,
}

/// Response for `GET /readyz`.
#[derive(Debug, Serialize)]
pub(crate) struct ReadyResponse {
    pub ok: bool,
    pub config_loaded: bool,
    pub redis_ok: bool,
    pub jwks_ok: bool,
    pub last_config_reload_unix: i64,
}

/// Response for `GET /config/status`.
#[derive(Debug, Serialize)]
pub(crate) struct ConfigStatusResponse {
    pub active_config_version: u32,
    pub active_config_sha256: String,
    pub last_reload_result: String,
    pub last_reload_error: Option<String>,
    pub last_reload_unix: i64,
}

/// Success response for `POST /config/reload`.
#[derive(Debug, Serialize)]
pub(crate) struct ReloadSuccessResponse {
    pub ok: bool,
    pub message: String,
    pub previous_sha256: String,
    pub new_sha256: String,
    pub reload_timestamp: i64,
}

/// Error response for `POST /config/reload`.
#[derive(Debug, Serialize)]
pub(crate) struct ReloadErrorResponse {
    pub ok: bool,
    pub message: String,
    pub error: String,
    pub reload_timestamp: i64,
}

/// Response for `GET /xds/status`.
#[derive(Debug, Serialize)]
pub(crate) struct XdsStatusResponse {
    pub enabled: bool,
    pub connected_envoys: usize,
    pub peers: Vec<crate::xds::EnvoyConnectionStatus>,
}
