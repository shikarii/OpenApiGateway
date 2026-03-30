use std::time::Instant;

/// In-memory token bucket for survivability mode fallback.
///
/// Each bucket tracks its own token count and refills based on elapsed
/// wall-clock time since the last refill.
pub(crate) struct LocalBucket {
    tokens: f64,
    capacity: f64,
    refill_rate: f64,
    last_refill: Instant,
}

impl LocalBucket {
    /// Create a new bucket starting at full capacity.
    pub(crate) fn new(capacity: f64, refill_rate: f64) -> Self {
        Self {
            tokens: capacity,
            capacity,
            refill_rate,
            last_refill: Instant::now(),
        }
    }

    /// Attempt to consume one token.
    ///
    /// Returns `(allowed, remaining_tokens, retry_after_ms)`.
    pub(crate) fn try_consume(&mut self) -> (bool, u64, u64) {
        self.refill();

        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            (true, self.tokens.floor() as u64, 0)
        } else {
            let deficit = 1.0 - self.tokens;
            let retry_ms = if self.refill_rate > 0.0 {
                ((deficit / self.refill_rate) * 1000.0).ceil() as u64
            } else {
                u64::MAX
            };
            (false, 0, retry_ms)
        }
    }

    fn refill(&mut self) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        let refilled = self.refill_rate * elapsed;
        self.tokens = (self.tokens + refilled).min(self.capacity);
        self.last_refill = now;
    }
}

/// Compute Redis TTL for a bucket: `ceil((capacity / refill_rate) * 2)`.
pub(crate) fn ttl_seconds(capacity: u64, refill_rate: f64) -> usize {
    if refill_rate <= 0.0 {
        return 3600; // fallback: 1 hour
    }
    ((capacity as f64 / refill_rate) * 2.0).ceil() as usize
}

/// Build a Redis key from components.
pub(crate) fn build_key(prefix: &str, route: &str, dimension: &str, value: &str) -> String {
    format!("{prefix}:{route}:{dimension}:{value}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bucket_starts_full() {
        let mut b = LocalBucket::new(10.0, 1.0);
        let (allowed, remaining, _) = b.try_consume();
        assert!(allowed);
        assert_eq!(remaining, 9);
    }

    #[test]
    fn bucket_exhausted_returns_denied() {
        let mut b = LocalBucket::new(3.0, 0.0);
        assert!(b.try_consume().0);
        assert!(b.try_consume().0);
        assert!(b.try_consume().0);
        let (allowed, remaining, retry) = b.try_consume();
        assert!(!allowed);
        assert_eq!(remaining, 0);
        assert_eq!(retry, u64::MAX); // zero refill rate
    }

    #[test]
    fn retry_after_ms_nonzero_when_denied() {
        let mut b = LocalBucket::new(1.0, 10.0);
        b.try_consume(); // depletes the single token
        let (allowed, _, retry) = b.try_consume();
        assert!(!allowed);
        assert!(retry > 0);
    }

    #[test]
    fn ttl_formula() {
        assert_eq!(ttl_seconds(50, 10.0), 10);
        assert_eq!(ttl_seconds(100, 10.0), 20);
        assert_eq!(ttl_seconds(1, 1.0), 2);
    }

    #[test]
    fn ttl_zero_refill_fallback() {
        assert_eq!(ttl_seconds(50, 0.0), 3600);
    }

    #[test]
    fn key_format() {
        let key = build_key("rl", "public-api", "ip", "192.168.1.20");
        assert_eq!(key, "rl:public-api:ip:192.168.1.20");
    }

    #[test]
    fn key_format_sub() {
        let key = build_key("rl", "private-api", "sub", "user-123");
        assert_eq!(key, "rl:private-api:sub:user-123");
    }

    #[test]
    fn capacity_one_single_token() {
        let mut b = LocalBucket::new(1.0, 0.0);
        let (allowed, remaining, _) = b.try_consume();
        assert!(allowed);
        assert_eq!(remaining, 0);
        let (allowed, _, retry) = b.try_consume();
        assert!(!allowed);
        assert_eq!(retry, u64::MAX);
    }

    #[test]
    fn remaining_decrements_each_consume() {
        let mut b = LocalBucket::new(5.0, 0.0);
        for expected in (0..5).rev() {
            let (allowed, remaining, _) = b.try_consume();
            assert!(allowed);
            assert_eq!(remaining, expected);
        }
        assert!(!b.try_consume().0);
    }

    #[test]
    fn retry_after_formula_accuracy() {
        // refill_rate=2.0 tokens/sec → deficit 1.0 takes 0.5s = 500ms
        let mut b = LocalBucket::new(1.0, 2.0);
        b.try_consume(); // consume the only token
        let (_, _, retry) = b.try_consume();
        // ceil((1.0 / 2.0) * 1000) = 500
        assert_eq!(retry, 500);
    }

    #[test]
    fn ttl_fractional_rounds_up() {
        // ttl_seconds(3, 2.0) = ceil((3.0/2.0)*2) = ceil(3.0) = 3
        assert_eq!(ttl_seconds(3, 2.0), 3);
        // ttl_seconds(5, 3.0) = ceil((5.0/3.0)*2) = ceil(3.333) = 4
        assert_eq!(ttl_seconds(5, 3.0), 4);
    }
}
