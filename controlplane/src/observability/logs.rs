use std::sync::atomic::{AtomicU64, Ordering};

/// A single access log entry serialized as one JSON line to stdout.
///
/// Every proxied request produces exactly one entry after the response
/// completes. Fields match the observability spec.
#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct AccessLogEntry {
    /// Request timestamp (RFC 3339, UTC, millisecond precision).
    pub ts: String,
    /// Unique request identifier (8-char hex).
    pub request_id: String,
    /// Client IP address.
    pub remote_addr: String,
    /// HTTP Host header value.
    pub host: String,
    /// HTTP method.
    pub method: String,
    /// Request path (no query string).
    pub path: String,
    /// Matched route name from config, or empty string for 404.
    pub route: String,
    /// HTTP response status code.
    pub status: u16,
    /// Total request duration in milliseconds.
    pub duration_ms: u64,
    /// Request body size in bytes.
    pub bytes_in: u64,
    /// Response body size in bytes.
    pub bytes_out: u64,
    /// JWT subject claim if authenticated, null if not.
    pub auth_subject: Option<String>,
    /// Rate limiting mode: "redis", "degraded-local", or "none".
    pub rate_limit_mode: String,
    /// Upstream service name from config.
    pub upstream_service: String,
    /// Upstream address used for this request.
    pub upstream_addr: String,
}

impl AccessLogEntry {
    /// Serialize this entry as a single JSON line and write to stdout.
    ///
    /// Errors are logged via tracing but never propagated — access log
    /// failures must not break request processing.
    pub(crate) fn emit(&self) {
        match serde_json::to_string(self) {
            Ok(json) => println!("{json}"),
            Err(e) => tracing::error!(error = %e, "failed to serialize access log entry"),
        }
    }

    /// Serialize this entry as a JSON string without writing.
    #[cfg(test)]
    pub(crate) fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }
}

/// Global counter for request ID generation.
static REQUEST_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Generate a short hex request ID (8 characters).
///
/// Combines a monotonic counter with the low bits of the current
/// timestamp to produce a compact, locally-unique identifier.
pub(crate) fn generate_request_id() -> String {
    let count = REQUEST_COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos() as u64;
    let mixed = count.wrapping_mul(2654435761) ^ nanos;
    format!("{:08x}", mixed as u32)
}

/// Current UTC timestamp as RFC 3339 with millisecond precision.
pub(crate) fn now_rfc3339() -> String {
    chrono::Utc::now()
        .format("%Y-%m-%dT%H:%M:%S%.3fZ")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_entry() -> AccessLogEntry {
        AccessLogEntry {
            ts: "2026-03-29T23:59:59.000Z".into(),
            request_id: "2c3c7b8f".into(),
            remote_addr: "172.18.0.1".into(),
            host: "localhost".into(),
            method: "GET".into(),
            path: "/private/data".into(),
            route: "private-echo".into(),
            status: 200,
            duration_ms: 14,
            bytes_in: 123,
            bytes_out: 456,
            auth_subject: Some("user-123".into()),
            rate_limit_mode: "redis".into(),
            upstream_service: "echo-private".into(),
            upstream_addr: "echo-backend:8081".into(),
        }
    }

    #[test]
    fn serializes_all_fields() {
        let entry = sample_entry();
        let json = entry.to_json().unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(v["ts"], "2026-03-29T23:59:59.000Z");
        assert_eq!(v["request_id"], "2c3c7b8f");
        assert_eq!(v["remote_addr"], "172.18.0.1");
        assert_eq!(v["host"], "localhost");
        assert_eq!(v["method"], "GET");
        assert_eq!(v["path"], "/private/data");
        assert_eq!(v["route"], "private-echo");
        assert_eq!(v["status"], 200);
        assert_eq!(v["duration_ms"], 14);
        assert_eq!(v["bytes_in"], 123);
        assert_eq!(v["bytes_out"], 456);
        assert_eq!(v["auth_subject"], "user-123");
        assert_eq!(v["rate_limit_mode"], "redis");
        assert_eq!(v["upstream_service"], "echo-private");
        assert_eq!(v["upstream_addr"], "echo-backend:8081");
    }

    #[test]
    fn null_auth_subject() {
        let mut entry = sample_entry();
        entry.auth_subject = None;
        let json = entry.to_json().unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(v["auth_subject"].is_null());
    }

    #[test]
    fn empty_route_on_404() {
        let mut entry = sample_entry();
        entry.route = "".into();
        entry.status = 404;
        entry.auth_subject = None;
        entry.rate_limit_mode = "none".into();
        entry.upstream_service = "".into();
        entry.upstream_addr = "".into();

        let json = entry.to_json().unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["route"], "");
        assert_eq!(v["status"], 404);
        assert_eq!(v["upstream_service"], "");
    }

    #[test]
    fn generate_request_id_length() {
        let id = generate_request_id();
        assert_eq!(id.len(), 8);
        // All hex characters.
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn generate_request_id_uniqueness() {
        let ids: Vec<String> = (0..1000).map(|_| generate_request_id()).collect();
        let mut deduped = ids.clone();
        deduped.sort();
        deduped.dedup();
        // At least 99% unique (collision possible with 32-bit space but very unlikely for 1000).
        assert!(
            deduped.len() >= 990,
            "too many collisions: {}",
            deduped.len()
        );
    }

    #[test]
    fn now_rfc3339_format() {
        let ts = now_rfc3339();
        // Should end with Z and contain T separator.
        assert!(ts.ends_with('Z'), "timestamp should end with Z: {ts}");
        assert!(ts.contains('T'), "timestamp should contain T: {ts}");
        // Should have millisecond precision (3 decimal places before Z).
        let dot_pos = ts.rfind('.').expect("should have decimal point");
        let frac = &ts[dot_pos + 1..ts.len() - 1]; // between . and Z
        assert_eq!(frac.len(), 3, "expected 3 decimal places, got: {frac}");
    }

    #[test]
    fn round_trip_json() {
        let entry = sample_entry();
        let json = entry.to_json().unwrap();
        // Should parse back to a valid JSON value.
        let _: serde_json::Value = serde_json::from_str(&json).unwrap();
    }
}
