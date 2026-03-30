use std::sync::Arc;
use std::time::{Duration, Instant};

use jsonwebtoken::DecodingKey;
use shared::config_types::AuthProvider;
use tokio::sync::{Notify, RwLock};

use super::error::AuthError;
use super::types::{JwksDocument, JwksKey};

/// JWKS cache for a single auth provider.
///
/// Fetches keys from the provider's `jwks_uri`, stores them behind a `RwLock`,
/// and runs a background refresh loop on a TTL interval.
pub(crate) struct JwksCache {
    provider: AuthProvider,
    inner: RwLock<CacheInner>,
    refresh_notify: Arc<Notify>,
    http: reqwest::Client,
}

struct CacheInner {
    keys: Vec<JwksKey>,
    fetched_at: Instant,
}

/// Stale multiplier: cache older than `STALE_FACTOR × cache_ttl_seconds` is stale.
const STALE_FACTOR: u64 = 10;

impl JwksCache {
    /// Create a new cache and perform the initial JWKS fetch.
    pub(crate) async fn new(
        provider: AuthProvider,
        http: reqwest::Client,
    ) -> Result<Arc<Self>, AuthError> {
        let cache = Arc::new(Self {
            provider,
            inner: RwLock::new(CacheInner {
                keys: Vec::new(),
                fetched_at: Instant::now(),
            }),
            refresh_notify: Arc::new(Notify::new()),
            http,
        });

        cache.fetch_and_store().await?;
        Ok(cache)
    }

    /// Return a snapshot of current cached keys.
    pub(crate) async fn get_keys(&self) -> Vec<JwksKey> {
        let inner = self.inner.read().await;
        inner
            .keys
            .iter()
            .map(|k| JwksKey {
                kid: k.kid.clone(),
                decoding_key: k.decoding_key.clone(),
            })
            .collect()
    }

    /// Signal the background loop to run an immediate out-of-schedule fetch.
    pub(crate) fn trigger_refresh(&self) {
        self.refresh_notify.notify_one();
    }

    /// Wait for the next refresh to complete. Use with a timeout.
    pub(crate) async fn wait_for_refresh(&self) {
        self.refresh_notify.notified().await;
    }

    /// True if the last successful fetch is older than `STALE_FACTOR × cache_ttl_seconds`.
    pub(crate) async fn is_stale(&self) -> bool {
        let inner = self.inner.read().await;
        let stale_threshold = Duration::from_secs(self.provider.cache_ttl_seconds * STALE_FACTOR);
        inner.fetched_at.elapsed() > stale_threshold
    }

    /// Provider name for registry lookup.
    pub(crate) fn provider_name(&self) -> &str {
        &self.provider.name
    }

    /// Fetch JWKS from the provider URI and update the cache.
    pub(crate) async fn fetch_and_store(&self) -> Result<(), AuthError> {
        let doc: JwksDocument = self
            .http
            .get(&self.provider.jwks_uri)
            .send()
            .await
            .map_err(|e| AuthError::JwksFetch(e.to_string()))?
            .json()
            .await
            .map_err(|e| AuthError::JwksFetch(e.to_string()))?;

        let keys = parse_jwks_keys(&doc)?;

        let mut inner = self.inner.write().await;
        inner.keys = keys;
        inner.fetched_at = Instant::now();

        tracing::debug!(
            provider = %self.provider.name,
            key_count = inner.keys.len(),
            "JWKS cache refreshed"
        );

        Ok(())
    }

    /// Spawn the background TTL refresh loop. Call once at startup.
    pub(crate) fn spawn_refresh_loop(self: Arc<Self>) {
        let ttl = Duration::from_secs(self.provider.cache_ttl_seconds);
        let cache = self;
        let notify = Arc::clone(&cache.refresh_notify);

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(ttl);
            // The first tick fires immediately; skip it since we just fetched.
            interval.tick().await;

            loop {
                tokio::select! {
                    _ = interval.tick() => {}
                    _ = notify.notified() => {}
                }

                if let Err(e) = cache.fetch_and_store().await {
                    tracing::warn!(
                        provider = %cache.provider.name,
                        error = %e,
                        "JWKS background refresh failed"
                    );
                }
                // Notify anyone waiting (e.g., unknown-kid retry path).
                notify.notify_waiters();
            }
        });
    }
}

/// Parse raw JWKS keys into `JwksKey` with `DecodingKey`.
fn parse_jwks_keys(doc: &JwksDocument) -> Result<Vec<JwksKey>, AuthError> {
    let mut keys = Vec::with_capacity(doc.keys.len());
    for raw in &doc.keys {
        if raw.kty != "RSA" {
            continue;
        }
        let decoding_key = DecodingKey::from_rsa_components(&raw.n, &raw.e).map_err(|e| {
            AuthError::JwksFetch(format!("failed to parse RSA key kid={}: {e}", raw.kid))
        })?;
        keys.push(JwksKey {
            kid: raw.kid.clone(),
            decoding_key,
        });
    }
    Ok(keys)
}

// Test-only constructor that doesn't require HTTP.
#[cfg(test)]
impl JwksCache {
    /// Build a cache with pre-loaded keys and a controlled fetch timestamp.
    ///
    /// The `http` client is a dummy — `fetch_and_store` should not be called.
    pub(crate) fn for_test(
        provider: AuthProvider,
        keys: Vec<JwksKey>,
        fetched_at: Instant,
    ) -> Arc<Self> {
        Arc::new(Self {
            provider,
            inner: RwLock::new(CacheInner { keys, fetched_at }),
            refresh_notify: Arc::new(Notify::new()),
            http: reqwest::Client::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::types::JwksRawKey;
    use super::*;

    #[test]
    fn parse_valid_jwks_keys() {
        // Use base64url-encoded RSA components (small test values).
        let doc = JwksDocument {
            keys: vec![JwksRawKey {
                kid: "key-1".into(),
                kty: "RSA".into(),
                alg: Some("RS256".into()),
                // These are valid base64url-encoded RSA public key components
                // from a real 2048-bit key (abbreviated for test fixture).
                n: "0vx7agoebGcQSuuPiLJXZptN9nndrQmbXEps2aiAFbWhM78LhWx4cbbfAAtVT86zwu1RK7aPFFxuhDR1L6tSoc_BJECPebWKRXjBZCiFV4n3oknjhMstn64tZ_2W-5JsGY4Hc5n9yBXArwl93lqt7_RN5w6Cf0h4QyQ5v-65YGjQR0_FDW2QvzqY368QQMicAtaSqzs8KJZgnYb9c7d0zgdAZHzu6qMQvRL5hajrn1n91CbOpbISD08qNLyrdkt-bFTWhAI4vMQFh6WeZu0fM4lFd2NcRwr3XPksINHaQ-G_xBniIqbw0Ls1jF44-csFCur-kEgU8awapJzKnqDKgw".into(),
                e: "AQAB".into(),
            }],
        };

        let keys = parse_jwks_keys(&doc).unwrap();
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].kid, "key-1");
    }

    #[test]
    fn skip_non_rsa_keys() {
        let doc = JwksDocument {
            keys: vec![JwksRawKey {
                kid: "ec-key".into(),
                kty: "EC".into(),
                alg: Some("ES256".into()),
                n: "".into(),
                e: "".into(),
            }],
        };

        let keys = parse_jwks_keys(&doc).unwrap();
        assert!(keys.is_empty());
    }

    fn test_provider(ttl: u64) -> AuthProvider {
        AuthProvider {
            name: "test-provider".into(),
            issuer: "https://auth.example.com/".into(),
            audience: "api-gateway".into(),
            jwks_uri: "http://localhost/.well-known/jwks.json".into(),
            cache_ttl_seconds: ttl,
            clock_skew_seconds: 30,
        }
    }

    fn test_decoding_key() -> DecodingKey {
        let pub_pem = include_bytes!("../../../test_fixtures/rsa_public.pem");
        DecodingKey::from_rsa_pem(pub_pem).expect("test public key")
    }

    #[tokio::test]
    async fn fresh_cache_is_not_stale() {
        let cache = JwksCache::for_test(test_provider(300), vec![], Instant::now());
        assert!(!cache.is_stale().await);
    }

    #[tokio::test]
    async fn old_cache_is_stale() {
        let old = Instant::now() - Duration::from_secs(3600);
        let cache = JwksCache::for_test(test_provider(1), vec![], old);
        // Stale threshold = 1 * 10 = 10s; elapsed ~3600s → stale.
        assert!(cache.is_stale().await);
    }

    #[tokio::test]
    async fn get_keys_returns_loaded_keys() {
        let key = JwksKey {
            kid: "k1".into(),
            decoding_key: test_decoding_key(),
        };
        let cache = JwksCache::for_test(test_provider(300), vec![key], Instant::now());
        let keys = cache.get_keys().await;
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].kid, "k1");
    }

    #[test]
    fn provider_name_returns_correct_name() {
        let cache = JwksCache::for_test(test_provider(300), vec![], Instant::now());
        assert_eq!(cache.provider_name(), "test-provider");
    }

    #[tokio::test]
    async fn trigger_and_wait_notify() {
        let cache = JwksCache::for_test(test_provider(300), vec![], Instant::now());
        // trigger_refresh notifies one waiter; spawn a waiter then trigger.
        let notify = Arc::clone(&cache.refresh_notify);
        let handle = tokio::spawn(async move {
            notify.notified().await;
            true
        });
        // Small yield to ensure the spawned task registers the waiter.
        tokio::task::yield_now().await;
        cache.trigger_refresh();
        let got = handle.await.expect("task panicked");
        assert!(got);
    }
}
