use std::path::PathBuf;
use std::sync::Arc;

use tokio::net::TcpListener;

mod admin;
mod auth;
mod config;
mod extauthz;
mod observability;
mod ratelimit;

#[tokio::main]
async fn main() {
    // Phase 1: Read config (use eprintln for errors -- tracing not yet initialized).
    let config_path = std::env::var("GATEWAY_CONFIG")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("configs/gateway.yaml"));

    let raw_yaml = match std::fs::read(&config_path) {
        Ok(bytes) => bytes,
        Err(e) => {
            eprintln!("failed to read {}: {e}", config_path.display());
            std::process::exit(1);
        }
    };

    let cfg = match config::load_config_from_str(&String::from_utf8_lossy(&raw_yaml)) {
        Ok(c) => c,
        Err(errs) => {
            eprintln!("config validation failed: {errs}");
            std::process::exit(1);
        }
    };

    // Phase 2: Initialize tracing (now that we have config).
    observability::init_tracing(&cfg.observability.tracing).unwrap_or_else(|e| {
        eprintln!("failed to initialize tracing: {e}");
        std::process::exit(1);
    });

    tracing::info!("controlplane starting");

    // Phase 2.5: Generate and write Envoy config (if path set).
    let envoy_config_path = std::env::var("ENVOY_CONFIG_PATH").ok().map(PathBuf::from);
    if let Some(ref envoy_path) = envoy_config_path {
        let envoy_yaml = config::generate_envoy_config(&cfg).unwrap_or_else(|e| {
            tracing::error!(error = %e, "failed to generate envoy config");
            std::process::exit(1);
        });
        std::fs::write(envoy_path, envoy_yaml).unwrap_or_else(|e| {
            tracing::error!(path = %envoy_path.display(), error = %e, "failed to write envoy config");
            std::process::exit(1);
        });
        tracing::info!(path = %envoy_path.display(), "envoy config written");
    }

    let admin_addr = cfg.gateway.admin_address.clone();
    let extauthz_addr = cfg.gateway.extauthz_address.clone();
    tracing::info!(
        version = cfg.version,
        routes = cfg.routes.len(),
        services = cfg.services.len(),
        "config loaded"
    );

    // Phase 3: Build subsystems.
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

    let metrics = Arc::new(observability::MetricsRegistry::new().unwrap_or_else(|e| {
        tracing::error!(error = %e, "failed to create metrics registry");
        std::process::exit(1);
    }));

    let state = admin::state::build_state(
        cfg,
        &raw_yaml,
        config_path,
        envoy_config_path,
        jwks_registry,
        rate_limiter,
        metrics,
    );

    // Phase 4: Start servers.
    let admin_app = admin::router(Arc::clone(&state));
    let admin_listener = TcpListener::bind(&admin_addr).await.unwrap_or_else(|e| {
        tracing::error!(%admin_addr, "failed to bind admin API: {e}");
        std::process::exit(1);
    });
    tracing::info!(%admin_addr, "admin API listening");

    if let Some(ref ea_addr) = extauthz_addr {
        let authz_app = extauthz::router(Arc::clone(&state));
        let authz_listener = TcpListener::bind(ea_addr).await.unwrap_or_else(|e| {
            tracing::error!(%ea_addr, "failed to bind ext_authz service: {e}");
            std::process::exit(1);
        });
        tracing::info!(addr = %ea_addr, "ext_authz service listening");

        tokio::select! {
            res = axum::serve(admin_listener, admin_app) => {
                if let Err(e) = res {
                    tracing::error!("admin API server error: {e}");
                }
            }
            res = axum::serve(authz_listener, authz_app) => {
                if let Err(e) = res {
                    tracing::error!("ext_authz server error: {e}");
                }
            }
        }
    } else {
        axum::serve(admin_listener, admin_app)
            .await
            .unwrap_or_else(|e| {
                tracing::error!("admin API server error: {e}");
                std::process::exit(1);
            });
    }

    observability::shutdown_tracing();
}
