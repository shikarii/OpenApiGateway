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
async fn survivability_priority_over_fail_open() {
    // Both enabled — survivability takes priority.
    let limiter = RateLimiter::offline_for_test(test_config(true, true));
    let d = limiter.check("rl:test:ip:10.0.0.1", 10, 1.0).await.unwrap();
    assert!(d.allowed);
    assert_eq!(d.mode, RateLimitMode::DegradedLocal);
}

#[tokio::test]
async fn different_keys_use_separate_buckets() {
    let limiter = RateLimiter::offline_for_test(test_config(false, true));

    // Exhaust key-A (capacity 5).
    for _ in 0..5 {
        limiter.check("key-a", 10, 1.0).await.unwrap();
    }
    let d = limiter.check("key-a", 10, 1.0).await.unwrap();
    assert!(!d.allowed, "key-a should be exhausted");

    // key-B should still have full capacity.
    let d = limiter.check("key-b", 10, 1.0).await.unwrap();
    assert!(d.allowed, "key-b should be independent");
}

#[tokio::test]
async fn fail_open_decision_fields() {
    let limiter = RateLimiter::offline_for_test(test_config(true, false));
    let d = limiter.check("rl:test:ip:1.2.3.4", 42, 5.0).await.unwrap();
    assert!(d.allowed);
    assert_eq!(d.limit, 42);
    assert_eq!(d.remaining, 42);
    assert_eq!(d.retry_after_ms, 0);
    assert_eq!(d.mode, RateLimitMode::FailOpen);
}

#[tokio::test]
async fn degraded_remaining_decrements() {
    let limiter = RateLimiter::offline_for_test(test_config(false, true));

    // Fallback capacity=5, consume 3 → remaining=2.
    for _ in 0..3 {
        limiter.check("rl:test:ip:1.1.1.1", 10, 1.0).await.unwrap();
    }
    let d = limiter.check("rl:test:ip:1.1.1.1", 10, 1.0).await.unwrap();
    assert!(d.allowed);
    assert_eq!(d.remaining, 1); // consumed 4th of 5
}

#[tokio::test]
async fn degraded_denied_retry_after_nonzero() {
    let limiter = RateLimiter::offline_for_test(test_config(false, true));

    // Exhaust all 5 tokens.
    for _ in 0..5 {
        limiter.check("rl:test:ip:2.2.2.2", 10, 1.0).await.unwrap();
    }
    let d = limiter.check("rl:test:ip:2.2.2.2", 10, 1.0).await.unwrap();
    assert!(!d.allowed);
    assert!(d.retry_after_ms > 0, "retry_after should be positive");
    assert_eq!(d.remaining, 0);
}
