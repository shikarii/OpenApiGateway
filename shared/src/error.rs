use thiserror::Error;

use crate::config_error::ConfigErrors;

#[derive(Debug, Error)]
pub enum GatewayError {
    #[error("invalid OpenAPI spec: {0}")]
    InvalidSpec(String),

    #[error("route not found: {method} {path}")]
    RouteNotFound { method: String, path: String },

    #[error("validation failed: {0}")]
    ValidationFailed(String),

    #[error("upstream error: {0}")]
    Upstream(String),

    #[error(transparent)]
    Config(#[from] ConfigErrors),
}
