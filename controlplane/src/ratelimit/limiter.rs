use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use redis::aio::MultiplexedConnection;
use shared::config_types::RateLimitsConfig;
use tokio::sync::{Mutex, RwLock};

use super::bucket::{ttl_seconds, LocalBucket};
use super::error::RateLimitError;
use super::lua::{LuaResponse, LUA_SCRIPT};

/// Decision returned by [`RateLimiter::check`].
#[derive(Debug)]
pub(crate) struct RateDecision {
    pub allowed: bool,
    pub limit: u64,
    pub remaining: u64,
    pub retry_after_ms: u64,
    pub mode: RateLimitMode,
}

/// Which backend served the rate-limit decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RateLimitMode {
    Redis,
    DegradedLocal,
    FailOpen,
}

impl RateLimitMode {
    pub(crate) fn as_header_value(self) -> &'static str {
        match self {
            Self::Redis | Self::FailOpen => "redis",
            Self::DegradedLocal => "degraded-local",
        }
    }
}

/// Token bucket rate limiter backed by Redis + Lua with in-memory fallback.
pub(crate) struct RateLimiter {
    config: RateLimitsConfig,
    client: redis::Client,
    conn: Arc<Mutex<Option<MultiplexedConnection>>>,
    script: redis::Script,
    fallback: RwLock<HashMap<String, LocalBucket>>,
}

impl RateLimiter {
    /// Build and attempt initial Redis connection.
    ///
    /// Never fails: if Redis is unreachable, the limiter starts in degraded mode.
    pub(crate) async fn from_config(config: &RateLimitsConfig) -> Arc<Self> {
        let url = format!("redis://{}/{}", config.redis_address, config.redis_db);
        let client = redis::Client::open(url.as_str()).unwrap_or_else(|e| {
            tracing::warn!(error = %e, "invalid redis URL, starting degraded");
            // Build a client pointing at a dummy address; it will fail on connect.
            redis::Client::open("redis://invalid:0/0").expect("dummy client")
        });

        let conn = match tokio::time::timeout(
            Duration::from_millis(config.default_timeout_ms),
            client.get_multiplexed_tokio_connection(),
        )
        .await
        {
            Ok(Ok(c)) => {
                tracing::info!("redis connected");
                Some(c)
            }
            Ok(Err(e)) => {
                tracing::warn!(error = %e, "redis connection failed, starting degraded");
                None
            }
            Err(_) => {
                tracing::warn!("redis connection timed out, starting degraded");
                None
            }
        };

        Arc::new(Self {
            config: config.clone(),
            client,
            conn: Arc::new(Mutex::new(conn)),
            script: redis::Script::new(LUA_SCRIPT),
            fallback: RwLock::new(HashMap::new()),
        })
    }

    /// Check (and deduct) one token for the given bucket key.
    pub(crate) async fn check(
        &self,
        bucket_key: &str,
        capacity: u64,
        refill_rate: f64,
    ) -> Result<RateDecision, RateLimitError> {
        match self.try_redis(bucket_key, capacity, refill_rate).await {
            Ok(decision) => Ok(decision),
            Err(_) => self.enter_degraded(bucket_key, capacity, refill_rate).await,
        }
    }

    /// Ping Redis. Returns `true` if reachable within the configured timeout.
    ///
    /// Also attempts to re-establish the connection if currently disconnected.
    pub(crate) async fn ping(&self) -> bool {
        let mut guard = self.conn.lock().await;

        // Try existing connection first.
        if let Some(ref mut conn) = *guard {
            let result = tokio::time::timeout(
                Duration::from_millis(self.config.default_timeout_ms),
                redis::cmd("PING").query_async::<String>(conn),
            )
            .await;
            if let Ok(Ok(_)) = result {
                return true;
            }
            // Connection is dead, drop it.
            *guard = None;
        }

        // Attempt reconnection.
        match tokio::time::timeout(
            Duration::from_millis(self.config.default_timeout_ms),
            self.client.get_multiplexed_tokio_connection(),
        )
        .await
        {
            Ok(Ok(new_conn)) => {
                *guard = Some(new_conn);
                true
            }
            _ => false,
        }
    }

    async fn try_redis(
        &self,
        bucket_key: &str,
        capacity: u64,
        refill_rate: f64,
    ) -> Result<RateDecision, RateLimitError> {
        let mut guard = self.conn.lock().await;
        let conn = guard
            .as_mut()
            .ok_or(RateLimitError::RedisConnect("no active connection".into()))?;

        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let ttl = ttl_seconds(capacity, refill_rate);

        let result = tokio::time::timeout(
            Duration::from_millis(self.config.default_timeout_ms),
            self.script
                .key(bucket_key)
                .arg(now_ms)
                .arg(capacity)
                .arg(refill_rate)
                .arg(1u64) // requested tokens
                .arg(ttl)
                .invoke_async::<String>(conn),
        )
        .await;

        match result {
            Ok(Ok(json_str)) => {
                let resp: LuaResponse = serde_json::from_str(&json_str)
                    .map_err(|e| RateLimitError::LuaResponseParse(e.to_string()))?;
                Ok(RateDecision {
                    allowed: resp.allowed == 1,
                    limit: capacity,
                    remaining: resp.remaining_tokens,
                    retry_after_ms: resp.retry_after_ms,
                    mode: RateLimitMode::Redis,
                })
            }
            Ok(Err(e)) => {
                // Redis command error — mark connection dead.
                *guard = None;
                Err(RateLimitError::RedisCommand(e.to_string()))
            }
            Err(_) => {
                *guard = None;
                Err(RateLimitError::Timeout)
            }
        }
    }

    async fn enter_degraded(
        &self,
        bucket_key: &str,
        capacity: u64,
        _refill_rate: f64,
    ) -> Result<RateDecision, RateLimitError> {
        let sm = &self.config.survivability_mode;

        if sm.enabled {
            let mut map = self.fallback.write().await;
            let bucket = map.entry(bucket_key.to_string()).or_insert_with(|| {
                LocalBucket::new(sm.fallback_capacity as f64, sm.fallback_refill_rate_per_sec)
            });

            let (allowed, remaining, retry_after_ms) = bucket.try_consume();
            return Ok(RateDecision {
                allowed,
                limit: sm.fallback_capacity,
                remaining,
                retry_after_ms,
                mode: RateLimitMode::DegradedLocal,
            });
        }

        if self.config.fail_open {
            tracing::warn!(key = %bucket_key, "redis unavailable, fail_open=true, allowing");
            return Ok(RateDecision {
                allowed: true,
                limit: capacity,
                remaining: capacity,
                retry_after_ms: 0,
                mode: RateLimitMode::FailOpen,
            });
        }

        Err(RateLimitError::Unavailable)
    }
}

#[cfg(test)]
impl RateLimiter {
    /// Create an offline limiter for tests (no Redis connection).
    pub(crate) fn offline_for_test(config: RateLimitsConfig) -> Arc<Self> {
        let client = redis::Client::open("redis://invalid:0/0").expect("dummy client for test");
        Arc::new(Self {
            config,
            client,
            conn: Arc::new(Mutex::new(None)),
            script: redis::Script::new(LUA_SCRIPT),
            fallback: RwLock::new(HashMap::new()),
        })
    }
}

#[cfg(test)]
mod tests {
    use shared::config_types::SurvivabilityMode;

    use super::*;

    fn test_config(fail_open: bool, survivability_enabled: bool) -> RateLimitsConfig {
        RateLimitsConfig {
            redis_address: "invalid:0".into(),
            redis_db: 0,
            redis_key_prefix: "rl".into(),
            default_timeout_ms: 50,
            fail_open,
            survivability_mode: SurvivabilityMode {
                enabled: survivability_enabled,
                fallback_capacity: 5,
                fallback_refill_rate_per_sec: 1.0,
            },
        }
    }

    #[tokio::test]
    async fn fail_open_allows_when_offline() {
        let limiter = RateLimiter::offline_for_test(test_config(true, false));
        let result = limiter.check("rl:test:ip:1.2.3.4", 10, 1.0).await;
        let decision = result.unwrap();
        assert!(decision.allowed);
        assert_eq!(decision.mode, RateLimitMode::FailOpen);
    }

    #[tokio::test]
    async fn fail_closed_returns_unavailable() {
        let limiter = RateLimiter::offline_for_test(test_config(false, false));
        let result = limiter.check("rl:test:ip:1.2.3.4", 10, 1.0).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), RateLimitError::Unavailable));
    }

    #[tokio::test]
    async fn survivability_mode_uses_local_bucket() {
        let limiter = RateLimiter::offline_for_test(test_config(false, true));

        // Fallback capacity is 5.
        for i in 0..5 {
            let d = limiter.check("rl:test:ip:1.2.3.4", 10, 1.0).await.unwrap();
            assert!(d.allowed, "request {i} should be allowed");
            assert_eq!(d.mode, RateLimitMode::DegradedLocal);
        }

        // 6th request should be denied.
        let d = limiter.check("rl:test:ip:1.2.3.4", 10, 1.0).await.unwrap();
        assert!(!d.allowed);
        assert_eq!(d.mode, RateLimitMode::DegradedLocal);
        assert!(d.retry_after_ms > 0);
    }

    #[tokio::test]
    async fn ping_offline_returns_false() {
        let limiter = RateLimiter::offline_for_test(test_config(true, false));
        assert!(!limiter.ping().await);
    }

    #[test]
    fn rate_limit_mode_header_values() {
        assert_eq!(RateLimitMode::Redis.as_header_value(), "redis");
        assert_eq!(
            RateLimitMode::DegradedLocal.as_header_value(),
            "degraded-local"
        );
        assert_eq!(RateLimitMode::FailOpen.as_header_value(), "redis");
    }
}
