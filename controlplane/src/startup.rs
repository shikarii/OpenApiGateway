use std::path::PathBuf;
use std::sync::Arc;

use tokio::net::TcpListener;

use crate::{admin, auth, config, extauthz, extproc, observability, plugins, ratelimit, xds};

pub(crate) async fn run() {
    let config_path = std::env::var("GATEWAY_CONFIG")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("configs/gateway.yaml"));

    let raw_yaml = match std::fs::read(&config_path) {
        Ok(bytes) => bytes,
        Err(error) => fatal_stderr(&format!(
            "failed to read {}: {error}",
            config_path.display()
        )),
    };

    let cfg = match config::load_config_from_str(&String::from_utf8_lossy(&raw_yaml)) {
        Ok(config) => config,
        Err(errors) => fatal_stderr(&format!("config validation failed: {errors}")),
    };

    observability::init_tracing(&cfg.observability.tracing)
        .unwrap_or_else(|error| fatal_stderr(&format!("failed to initialize tracing: {error}")));

    let envoy_config_path = std::env::var("ENVOY_CONFIG_PATH").ok().map(PathBuf::from);
    if let Some(ref envoy_path) = envoy_config_path {
        write_envoy_config(&cfg, envoy_path);
    }

    tracing::info!("controlplane starting");
    tracing::info!(
        version = cfg.version,
        routes = cfg.routes.len(),
        services = cfg.services.len(),
        "config loaded"
    );

    let jwks_registry = init_jwks(&cfg).await;
    let rate_limiter = init_rate_limiter(&cfg).await;
    let metrics = init_metrics();
    let plugin_engine = init_plugins(&cfg);
    let xds = init_xds(&cfg, Arc::clone(&metrics));

    let state = admin::state::build_state(
        cfg,
        &raw_yaml,
        config_path,
        envoy_config_path,
        jwks_registry,
        rate_limiter,
        metrics,
        plugin_engine,
        xds.clone(),
    );

    serve(state, xds).await;
    observability::shutdown_tracing();
}

async fn init_jwks(config: &shared::config_types::GatewayConfig) -> Arc<auth::JwksCacheRegistry> {
    let registry = auth::JwksCacheRegistry::from_config(&config.auth, reqwest::Client::new())
        .await
        .unwrap_or_else(|error| fatal_log("JWKS initial fetch failed", &error.to_string()));
    let registry = Arc::new(registry);
    registry.spawn_all_refresh_loops();
    registry
}

async fn init_rate_limiter(
    config: &shared::config_types::GatewayConfig,
) -> Arc<ratelimit::RateLimiter> {
    let limiter = ratelimit::RateLimiter::from_config(&config.rate_limits).await;
    tracing::info!(
        redis_reachable = limiter.ping().await,
        "rate limiter initialized"
    );
    limiter
}

fn init_metrics() -> Arc<observability::MetricsRegistry> {
    Arc::new(
        observability::MetricsRegistry::new().unwrap_or_else(|error| {
            fatal_log("failed to create metrics registry", &error.to_string())
        }),
    )
}

fn init_plugins(
    config: &shared::config_types::GatewayConfig,
) -> Option<Arc<plugins::PluginEngine>> {
    plugins::PluginEngine::from_config(config)
        .map(Arc::new)
        .map(Some)
        .unwrap_or_else(|error| fatal_log("failed to initialize plugin engine", &error.to_string()))
}

fn init_xds(
    config: &shared::config_types::GatewayConfig,
    metrics: Arc<observability::MetricsRegistry>,
) -> Option<Arc<xds::XdsControlPlane>> {
    config.xds.enabled.then(|| {
        xds::XdsControlPlane::new(config, metrics).unwrap_or_else(|error| {
            fatal_log("failed to initialize xDS control plane", &error.to_string())
        })
    })
}

fn write_envoy_config(config: &shared::config_types::GatewayConfig, envoy_path: &PathBuf) {
    let envoy_yaml = config::generate_envoy_config(config)
        .unwrap_or_else(|error| fatal_log("failed to generate envoy config", &error.to_string()));
    std::fs::write(envoy_path, envoy_yaml).unwrap_or_else(|error| {
        fatal_log(
            &format!("failed to write envoy config to {}", envoy_path.display()),
            &error.to_string(),
        )
    });
    tracing::info!(path = %envoy_path.display(), "envoy config written");
}

async fn serve(state: admin::state::SharedState, xds: Option<Arc<xds::XdsControlPlane>>) {
    let admin_addr = state
        .config_state
        .read()
        .await
        .config
        .gateway
        .admin_address
        .clone();
    let extauthz_addr = state
        .config_state
        .read()
        .await
        .config
        .gateway
        .extauthz_address
        .clone();
    let ext_proc_addr = state
        .config_state
        .read()
        .await
        .config
        .ext_proc
        .listen_address
        .clone();
    let ext_proc_enabled = state.config_state.read().await.config.ext_proc.enabled;
    let xds_addr = state
        .config_state
        .read()
        .await
        .config
        .xds
        .listen_address
        .clone();

    let admin_app = admin::router(Arc::clone(&state));
    let admin_listener = TcpListener::bind(&admin_addr)
        .await
        .unwrap_or_else(|error| fatal_log("failed to bind admin API", &error.to_string()));
    tracing::info!(%admin_addr, "admin API listening");

    let mut servers = tokio::task::JoinSet::new();
    servers.spawn(async move {
        axum::serve(admin_listener, admin_app)
            .await
            .map_err(|error| format!("admin API server error: {error}"))
    });

    if let Some(addr) = extauthz_addr.as_ref() {
        let authz_app = extauthz::router(Arc::clone(&state));
        let authz_listener = TcpListener::bind(addr).await.unwrap_or_else(|error| {
            fatal_log("failed to bind ext_authz service", &error.to_string())
        });
        tracing::info!(addr = %addr, "ext_authz service listening");
        servers.spawn(async move {
            axum::serve(authz_listener, authz_app)
                .await
                .map_err(|error| format!("ext_authz server error: {error}"))
        });
    }

    if let Some(xds) = xds {
        let addr = parse_socket_addr(&xds_addr, "xDS");
        tracing::info!(addr = %addr, "xDS server listening");
        servers.spawn(async move {
            tonic::transport::Server::builder()
                .add_service(xds.ads_service())
                .serve(addr)
                .await
                .map_err(|error| format!("xDS server error: {error}"))
        });
    }

    if ext_proc_enabled {
        let addr = parse_socket_addr(&ext_proc_addr, "ext_proc");
        let ext_proc = extproc::ExtProcService::new();
        tracing::info!(addr = %addr, "ext_proc server listening");
        servers.spawn(async move {
            tonic::transport::Server::builder()
                .add_service(ext_proc.server())
                .serve(addr)
                .await
                .map_err(|error| format!("ext_proc server error: {error}"))
        });
    }

    if let Some(result) = servers.join_next().await {
        match result {
            Ok(Err(error)) => fatal_log("server exited with error", &error),
            Err(error) => fatal_log("server task panicked", &error.to_string()),
            Ok(Ok(())) => {}
        }
    }
}

fn parse_socket_addr(value: &str, label: &str) -> std::net::SocketAddr {
    value
        .parse::<std::net::SocketAddr>()
        .unwrap_or_else(|error| {
            fatal_log(
                &format!("invalid {label} listen address"),
                &error.to_string(),
            )
        })
}

fn fatal_stderr(message: &str) -> ! {
    eprintln!("{message}");
    std::process::exit(1);
}

fn fatal_log(message: &str, error: &str) -> ! {
    tracing::error!(error, "{message}");
    std::process::exit(1);
}
