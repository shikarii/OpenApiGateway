use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};
use shared::config_types::GatewayConfig;
use tokio::sync::{RwLock, Semaphore};

use crate::auth::JwksCacheRegistry;
use crate::observability::MetricsRegistry;
use crate::ratelimit::RateLimiter;

/// Shared application state for the admin API.
pub(crate) type SharedState = Arc<AppState>;

/// Top-level state container.
pub(crate) struct AppState {
    pub config_state: RwLock<ConfigState>,
    pub config_path: PathBuf,
    pub envoy_config_path: Option<PathBuf>,
    pub jwks_registry: Arc<JwksCacheRegistry>,
    pub rate_limiter: Arc<RateLimiter>,
    pub metrics: Arc<MetricsRegistry>,
    pub concurrency_limit: Arc<Semaphore>,
}

/// Mutable config state protected by a `RwLock`.
#[derive(Clone)]
pub(crate) struct ConfigState {
    pub config: GatewayConfig,
    pub sha256: String,
    pub last_reload_unix: i64,
    pub last_reload_result: ReloadResult,
    pub last_reload_error: Option<String>,
}

/// Result of the last config reload attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReloadResult {
    Success,
    ValidationError,
}

impl ReloadResult {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::ValidationError => "validation_error",
        }
    }
}

/// Compute SHA256 hex digest for raw config bytes.
pub(crate) fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}

/// Current Unix timestamp in seconds.
pub(crate) fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

/// Build initial [`AppState`] from a loaded config and its raw YAML bytes.
pub(crate) fn build_state(
    config: GatewayConfig,
    raw_yaml: &[u8],
    config_path: PathBuf,
    envoy_config_path: Option<PathBuf>,
    jwks_registry: Arc<JwksCacheRegistry>,
    rate_limiter: Arc<RateLimiter>,
    metrics: Arc<MetricsRegistry>,
) -> SharedState {
    let sha256 = sha256_hex(raw_yaml);
    let now = now_unix();
    let max_conc = config
        .gateway
        .max_concurrent_requests
        .map(|n| n as usize)
        .unwrap_or(Semaphore::MAX_PERMITS);
    let concurrency_limit = Arc::new(Semaphore::new(max_conc));

    Arc::new(AppState {
        config_state: RwLock::new(ConfigState {
            config,
            sha256,
            last_reload_unix: now,
            last_reload_result: ReloadResult::Success,
            last_reload_error: None,
        }),
        config_path,
        envoy_config_path,
        jwks_registry,
        rate_limiter,
        metrics,
        concurrency_limit,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_deterministic() {
        let hash = sha256_hex(b"hello world");
        assert_eq!(
            hash,
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
    }

    #[test]
    fn now_unix_positive() {
        assert!(now_unix() > 0);
    }
}
