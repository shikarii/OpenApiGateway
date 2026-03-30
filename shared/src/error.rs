use thiserror::Error;

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
}
