use std::path::PathBuf;

use tracing_subscriber::EnvFilter;

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

    match config::load_config(&config_path) {
        Ok(cfg) => {
            tracing::info!(
                version = cfg.version,
                routes = cfg.routes.len(),
                services = cfg.services.len(),
                "config loaded"
            );
        }
        Err(errs) => {
            tracing::error!(%errs, "config validation failed");
            std::process::exit(1);
        }
    }
}
