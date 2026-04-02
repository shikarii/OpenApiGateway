// Observability subsystem: Prometheus metrics, JSON access logs,
// optional OTLP distributed tracing.
mod logs;
pub(crate) mod metrics;
#[allow(dead_code)]
mod tracing_init;

pub(crate) use logs::{generate_request_id, now_rfc3339, AccessLogEntry};
pub(crate) use metrics::MetricsRegistry;
pub(crate) use tracing_init::{init_tracing, shutdown_tracing};

#[cfg(test)]
#[path = "observability_tests.rs"]
mod observability_tests;
