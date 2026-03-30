use std::path::PathBuf;
use std::sync::Arc;

use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;

mod admin;
mod auth;
mod config;
mod ratelimit;

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

    let http_client = reqwest::Client::new();
    let jwks_registry = match auth::JwksCacheRegistry::from_config(&cfg.auth, http_client).await {
        Ok(r) => Arc::new(r),
        Err(e) => {
            tracing::error!(error = %e, "JWKS initial fetch failed");
            std::process::exit(1);
        }
    };
    jwks_registry.spawn_all_refresh_loops();

    let rate_limiter = ratelimit::RateLimiter::from_config(&cfg.rate_limits).await;
    tracing::info!(
        redis_reachable = rate_limiter.ping().await,
        "rate limiter initialized"
    );

    let state = admin::state::build_state(cfg, &raw_yaml, config_path, jwks_registry, rate_limiter);
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
