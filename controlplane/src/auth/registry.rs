use std::collections::HashMap;
use std::sync::Arc;

use shared::config_types::AuthConfig;

use super::error::AuthError;
use super::jwks_cache::JwksCache;

/// Registry of per-provider JWKS caches.
///
/// Owns one [`JwksCache`] per configured auth provider, keyed by provider name.
pub(crate) struct JwksCacheRegistry {
    caches: HashMap<String, Arc<JwksCache>>,
}

impl JwksCacheRegistry {
    /// Build the registry by fetching JWKS for every configured provider.
    ///
    /// Fails if any provider's initial fetch fails (fail-safe at startup).
    pub(crate) async fn from_config(
        auth: &AuthConfig,
        http: reqwest::Client,
    ) -> Result<Self, AuthError> {
        let mut caches = HashMap::with_capacity(auth.providers.len());
        for provider in &auth.providers {
            let cache = JwksCache::new(provider.clone(), http.clone()).await?;
            caches.insert(provider.name.clone(), cache);
        }
        Ok(Self { caches })
    }

    /// Returns `true` if all provider caches are within their stale threshold.
    pub(crate) async fn all_healthy(&self) -> bool {
        for cache in self.caches.values() {
            if cache.is_stale().await {
                return false;
            }
        }
        true
    }

    /// Look up a provider cache by name.
    #[allow(dead_code)]
    pub(crate) fn get(&self, name: &str) -> Option<&Arc<JwksCache>> {
        self.caches.get(name)
    }

    /// Spawn background refresh loops for all providers.
    pub(crate) fn spawn_all_refresh_loops(&self) {
        for cache in self.caches.values() {
            Arc::clone(cache).spawn_refresh_loop();
        }
    }
}

// Test-only constructor that doesn't require HTTP.
#[cfg(test)]
impl JwksCacheRegistry {
    /// Create an empty registry where `all_healthy()` returns `true`.
    ///
    /// Used by admin handler tests that don't need real JWKS state.
    pub(crate) fn empty_for_test() -> Arc<Self> {
        Arc::new(Self {
            caches: HashMap::new(),
        })
    }

    /// Create a registry from pre-built caches (no HTTP fetch).
    pub(crate) fn from_caches_for_test(caches: HashMap<String, Arc<JwksCache>>) -> Self {
        Self { caches }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn empty_registry_is_healthy() {
        let registry = JwksCacheRegistry::empty_for_test();
        assert!(registry.all_healthy().await);
    }

    #[test]
    fn get_unknown_provider_returns_none() {
        let registry = JwksCacheRegistry {
            caches: HashMap::new(),
        };
        assert!(registry.get("nonexistent").is_none());
    }

    fn test_provider(name: &str, ttl: u64) -> shared::config_types::AuthProvider {
        shared::config_types::AuthProvider {
            name: name.into(),
            issuer: "https://auth.example.com/".into(),
            audience: "api-gateway".into(),
            jwks_uri: "http://localhost/.well-known/jwks.json".into(),
            cache_ttl_seconds: ttl,
            clock_skew_seconds: 30,
        }
    }

    #[tokio::test]
    async fn all_healthy_with_fresh_caches() {
        let mut caches = HashMap::new();
        let now = std::time::Instant::now();
        caches.insert(
            "a".into(),
            JwksCache::for_test(test_provider("a", 300), vec![], now),
        );
        caches.insert(
            "b".into(),
            JwksCache::for_test(test_provider("b", 300), vec![], now),
        );
        let reg = JwksCacheRegistry::from_caches_for_test(caches);
        assert!(reg.all_healthy().await);
    }

    #[tokio::test]
    async fn unhealthy_when_any_cache_stale() {
        let mut caches = HashMap::new();
        let now = std::time::Instant::now();
        let old = now - std::time::Duration::from_secs(3600);
        caches.insert(
            "fresh".into(),
            JwksCache::for_test(test_provider("fresh", 300), vec![], now),
        );
        caches.insert(
            "stale".into(),
            JwksCache::for_test(test_provider("stale", 1), vec![], old),
        );
        let reg = JwksCacheRegistry::from_caches_for_test(caches);
        assert!(!reg.all_healthy().await);
    }

    #[test]
    fn get_returns_existing_provider() {
        let mut caches = HashMap::new();
        caches.insert(
            "my-provider".into(),
            JwksCache::for_test(
                test_provider("my-provider", 300),
                vec![],
                std::time::Instant::now(),
            ),
        );
        let reg = JwksCacheRegistry::from_caches_for_test(caches);
        assert!(reg.get("my-provider").is_some());
        assert_eq!(
            reg.get("my-provider").unwrap().provider_name(),
            "my-provider"
        );
    }
}
