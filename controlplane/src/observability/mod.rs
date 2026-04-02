// Observability subsystem: Prometheus metrics, JSON access logs,
// optional OTLP distributed tracing.
#[allow(dead_code)]
mod logs;
#[allow(dead_code)]
pub(crate) mod metrics;
#[allow(dead_code)]
mod tracing_init;

#[allow(dead_code, unused_imports)]
pub(crate) use logs::{generate_request_id, now_rfc3339, AccessLogEntry};
pub(crate) use metrics::MetricsRegistry;
pub(crate) use tracing_init::{init_tracing, shutdown_tracing};

#[cfg(test)]
#[path = "observability_tests.rs"]
mod observability_tests;
