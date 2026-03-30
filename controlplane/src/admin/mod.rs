mod handlers;
pub(crate) mod responses;
pub(crate) mod state;

use axum::Router;

use self::state::SharedState;

/// Build the admin API router with all endpoints.
pub(crate) fn router(state: SharedState) -> Router {
    Router::new()
        .route("/healthz", axum::routing::get(handlers::healthz))
        .route("/readyz", axum::routing::get(handlers::readyz))
        .route(
            "/config/status",
            axum::routing::get(handlers::config_status),
        )
        .route(
            "/config/reload",
            axum::routing::post(handlers::config_reload),
        )
        .route("/metrics", axum::routing::get(handlers::metrics))
        .with_state(state)
}
