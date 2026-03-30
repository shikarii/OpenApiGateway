use std::path::PathBuf;

use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;

mod admin;
mod config;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    tracing::info!("controlplane starting");

    let config_path = std::env::var("GATEWAY_CONFIG")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("configs/gateway.yaml"));

    let raw_yaml = match std::fs::read(&config_path) {
        Ok(bytes) => bytes,
        Err(e) => {
            tracing::error!("failed to read {}: {e}", config_path.display());
            std::process::exit(1);
        }
    };

    let cfg = match config::load_config_from_str(&String::from_utf8_lossy(&raw_yaml)) {
        Ok(c) => c,
        Err(errs) => {
            tracing::error!(%errs, "config validation failed");
            std::process::exit(1);
        }
    };

    let admin_addr = cfg.gateway.admin_address.clone();
    tracing::info!(
        version = cfg.version,
        routes = cfg.routes.len(),
        services = cfg.services.len(),
        "config loaded"
    );

    let state = admin::state::build_state(cfg, &raw_yaml, config_path);
    let app = admin::router(state);

    let listener = TcpListener::bind(&admin_addr).await.unwrap_or_else(|e| {
        tracing::error!(%admin_addr, "failed to bind admin API: {e}");
        std::process::exit(1);
    });

    tracing::info!(%admin_addr, "admin API listening");
    axum::serve(listener, app).await.unwrap_or_else(|e| {
        tracing::error!("admin API server error: {e}");
        std::process::exit(1);
    });
}
