use prometheus::{
    CounterVec, Encoder, HistogramOpts, HistogramVec, IntCounter, IntGauge, Opts, Registry,
    TextEncoder,
};

/// Errors from the metrics subsystem.
#[derive(Debug, thiserror::Error)]
pub(crate) enum MetricsError {
    #[error("prometheus registration error: {0}")]
    Registration(#[from] prometheus::Error),
    #[error("metrics encoding error: {0}")]
    Encoding(String),
}

/// Convert HTTP status code to class string (e.g., 200 → "2xx").
fn status_class(status: u16) -> &'static str {
    match status {
        100..=199 => "1xx",
        200..=299 => "2xx",
        300..=399 => "3xx",
        400..=499 => "4xx",
        _ => "5xx",
    }
}

/// Prometheus metrics registry holding all gateway metrics.
///
/// All counter and histogram operations are internally atomic, so no
/// external locking is needed. Store as `Arc<MetricsRegistry>` in app state.
#[derive(Debug)]
pub(crate) struct MetricsRegistry {
    registry: Registry,
    http_requests_total: CounterVec,
    http_request_duration_ms: HistogramVec,
    auth_failures_total: CounterVec,
    rate_limit_allowed_total: CounterVec,
    rate_limit_denied_total: CounterVec,
    rate_limit_degraded_total: CounterVec,
    #[allow(dead_code)]
    upstream_failures_total: CounterVec,
    config_reload_total: CounterVec,
    inflight_requests: IntGauge,
    overload_total: IntCounter,
    plugin_duration_us: HistogramVec,
    plugin_errors_total: CounterVec,
    plugin_chain_duration_us: HistogramVec,
    plugin_short_circuits_total: CounterVec,
    xds_connected_envoys: IntGauge,
    xds_acks_total: IntCounter,
    xds_nacks_total: IntCounter,
    xds_snapshot_version: IntGauge,
    xds_push_duration_seconds: prometheus::Histogram,
}

/// Histogram buckets for request duration (milliseconds).
const DURATION_BUCKETS: &[f64] = &[
    1.0, 5.0, 10.0, 25.0, 50.0, 100.0, 250.0, 500.0, 1000.0, 5000.0,
];

impl MetricsRegistry {
    /// Create a new metrics registry with all gateway metrics registered.
    pub(crate) fn new() -> Result<Self, MetricsError> {
        let registry = Registry::new();

        let http_requests_total = CounterVec::new(
            Opts::new(
                "gateway_http_requests_total",
                "Total HTTP requests processed",
            ),
            &["route", "method", "status_class"],
        )?;

        let http_request_duration_ms = HistogramVec::new(
            HistogramOpts::new(
                "gateway_http_request_duration_ms",
                "Request duration in milliseconds",
            )
            .buckets(DURATION_BUCKETS.to_vec()),
            &["route"],
        )?;

        let auth_failures_total = CounterVec::new(
            Opts::new("gateway_auth_failures_total", "Authentication failures"),
            &["route", "reason"],
        )?;

        let rate_limit_allowed_total = CounterVec::new(
            Opts::new(
                "gateway_rate_limit_allowed_total",
                "Requests allowed by rate limiter",
            ),
            &["route"],
        )?;

        let rate_limit_denied_total = CounterVec::new(
            Opts::new(
                "gateway_rate_limit_denied_total",
                "Requests denied by rate limiter",
            ),
            &["route"],
        )?;

        let rate_limit_degraded_total = CounterVec::new(
            Opts::new(
                "gateway_rate_limit_degraded_total",
                "Rate limiting served from degraded fallback",
            ),
            &["route"],
        )?;

        let upstream_failures_total = CounterVec::new(
            Opts::new(
                "gateway_upstream_failures_total",
                "Upstream service failures",
            ),
            &["route", "service", "reason"],
        )?;

        let config_reload_total = CounterVec::new(
            Opts::new(
                "gateway_config_reload_total",
                "Configuration reload attempts",
            ),
            &["result"],
        )?;

        let inflight_requests = IntGauge::new(
            "gateway_inflight_requests",
            "Current in-flight HTTP requests",
        )?;

        let overload_total = IntCounter::new(
            "gateway_overload_total",
            "Requests rejected due to gateway overload",
        )?;

        let plugin_duration_us = HistogramVec::new(
            HistogramOpts::new(
                "gateway_plugin_duration_us",
                "Per-plugin execution time in microseconds",
            ),
            &["plugin", "route", "phase"],
        )?;

        let plugin_errors_total = CounterVec::new(
            Opts::new("gateway_plugin_errors_total", "Plugin runtime failures"),
            &["plugin", "route", "error_type"],
        )?;

        let plugin_chain_duration_us = HistogramVec::new(
            HistogramOpts::new(
                "gateway_plugin_chain_duration_us",
                "Total plugin chain duration in microseconds",
            ),
            &["route"],
        )?;

        let plugin_short_circuits_total = CounterVec::new(
            Opts::new(
                "gateway_plugin_short_circuits_total",
                "Requests short-circuited by plugins",
            ),
            &["plugin", "route", "status"],
        )?;

        let xds_connected_envoys = IntGauge::new(
            "xds_connected_envoys",
            "Currently connected Envoy xDS clients",
        )?;
        let xds_acks_total = IntCounter::new("xds_acks_total", "xDS ACKs received")?;
        let xds_nacks_total = IntCounter::new("xds_nacks_total", "xDS NACKs received")?;
        let xds_snapshot_version =
            IntGauge::new("xds_snapshot_version", "Current xDS snapshot version")?;
        let xds_push_duration_seconds = prometheus::Histogram::with_opts(HistogramOpts::new(
            "xds_push_duration_seconds",
            "xDS push broadcast duration in seconds",
        ))?;

        registry.register(Box::new(http_requests_total.clone()))?;
        registry.register(Box::new(http_request_duration_ms.clone()))?;
        registry.register(Box::new(auth_failures_total.clone()))?;
        registry.register(Box::new(rate_limit_allowed_total.clone()))?;
        registry.register(Box::new(rate_limit_denied_total.clone()))?;
        registry.register(Box::new(rate_limit_degraded_total.clone()))?;
        registry.register(Box::new(upstream_failures_total.clone()))?;
        registry.register(Box::new(config_reload_total.clone()))?;
        registry.register(Box::new(inflight_requests.clone()))?;
        registry.register(Box::new(overload_total.clone()))?;
        registry.register(Box::new(plugin_duration_us.clone()))?;
        registry.register(Box::new(plugin_errors_total.clone()))?;
        registry.register(Box::new(plugin_chain_duration_us.clone()))?;
        registry.register(Box::new(plugin_short_circuits_total.clone()))?;
        registry.register(Box::new(xds_connected_envoys.clone()))?;
        registry.register(Box::new(xds_acks_total.clone()))?;
        registry.register(Box::new(xds_nacks_total.clone()))?;
        registry.register(Box::new(xds_snapshot_version.clone()))?;
        registry.register(Box::new(xds_push_duration_seconds.clone()))?;

        Ok(Self {
            registry,
            http_requests_total,
            http_request_duration_ms,
            auth_failures_total,
            rate_limit_allowed_total,
            rate_limit_denied_total,
            rate_limit_degraded_total,
            upstream_failures_total,
            config_reload_total,
            inflight_requests,
            overload_total,
            plugin_duration_us,
            plugin_errors_total,
            plugin_chain_duration_us,
            plugin_short_circuits_total,
            xds_connected_envoys,
            xds_acks_total,
            xds_nacks_total,
            xds_snapshot_version,
            xds_push_duration_seconds,
        })
    }

    /// Record an HTTP request completion.
    pub(crate) fn record_request(&self, route: &str, method: &str, status: u16, duration_ms: f64) {
        let class = status_class(status);
        self.http_requests_total
            .with_label_values(&[route, method, class])
            .inc();
        self.http_request_duration_ms
            .with_label_values(&[route])
            .observe(duration_ms);
    }

    /// Record an authentication failure.
    pub(crate) fn record_auth_failure(&self, route: &str, reason: &str) {
        self.auth_failures_total
            .with_label_values(&[route, reason])
            .inc();
    }

    /// Record a rate limit allow decision.
    pub(crate) fn record_rate_limit_allowed(&self, route: &str) {
        self.rate_limit_allowed_total
            .with_label_values(&[route])
            .inc();
    }

    /// Record a rate limit deny decision.
    pub(crate) fn record_rate_limit_denied(&self, route: &str) {
        self.rate_limit_denied_total
            .with_label_values(&[route])
            .inc();
    }

    /// Record a rate limit degraded decision.
    pub(crate) fn record_rate_limit_degraded(&self, route: &str) {
        self.rate_limit_degraded_total
            .with_label_values(&[route])
            .inc();
    }

    /// Record an upstream service failure.
    #[allow(dead_code)]
    pub(crate) fn record_upstream_failure(&self, route: &str, service: &str, reason: &str) {
        self.upstream_failures_total
            .with_label_values(&[route, service, reason])
            .inc();
    }

    /// Record a config reload attempt.
    pub(crate) fn record_config_reload(&self, result: &str) {
        self.config_reload_total.with_label_values(&[result]).inc();
    }

    /// Increment in-flight request gauge.
    pub(crate) fn inc_inflight(&self) {
        self.inflight_requests.inc();
    }

    /// Decrement in-flight request gauge.
    pub(crate) fn dec_inflight(&self) {
        self.inflight_requests.dec();
    }

    /// Record a gateway overload rejection.
    pub(crate) fn record_overload(&self) {
        self.overload_total.inc();
    }

    /// Record one plugin invocation duration.
    pub(crate) fn record_plugin_duration(
        &self,
        plugin: &str,
        route: &str,
        phase: &str,
        duration_us: u64,
    ) {
        self.plugin_duration_us
            .with_label_values(&[plugin, route, phase])
            .observe(duration_us as f64);
    }

    /// Record a plugin error.
    pub(crate) fn record_plugin_error(&self, plugin: &str, route: &str, error_type: &str) {
        self.plugin_errors_total
            .with_label_values(&[plugin, route, error_type])
            .inc();
    }

    /// Record total chain duration.
    pub(crate) fn record_plugin_chain_duration(&self, route: &str, duration_us: u64) {
        self.plugin_chain_duration_us
            .with_label_values(&[route])
            .observe(duration_us as f64);
    }

    /// Record a plugin short-circuit response.
    pub(crate) fn record_plugin_short_circuit(&self, plugin: &str, route: &str, status: u16) {
        self.plugin_short_circuits_total
            .with_label_values(&[plugin, route, &status.to_string()])
            .inc();
    }

    /// Update the connected Envoy gauge.
    pub(crate) fn set_xds_connected_envoys(&self, count: i64) {
        self.xds_connected_envoys.set(count);
    }

    /// Record an xDS ACK.
    pub(crate) fn record_xds_ack(&self) {
        self.xds_acks_total.inc();
    }

    /// Record an xDS NACK.
    pub(crate) fn record_xds_nack(&self) {
        self.xds_nacks_total.inc();
    }

    /// Update the published xDS snapshot version.
    pub(crate) fn set_xds_snapshot_version(&self, version: i64) {
        self.xds_snapshot_version.set(version);
    }

    /// Record xDS push duration.
    pub(crate) fn record_xds_push(&self, duration_seconds: f64) {
        self.xds_push_duration_seconds.observe(duration_seconds);
    }

    /// Encode all metrics as Prometheus text exposition format.
    pub(crate) fn encode(&self) -> Result<String, MetricsError> {
        let encoder = TextEncoder::new();
        let families = self.registry.gather();
        let mut buf = Vec::new();
        encoder
            .encode(&families, &mut buf)
            .map_err(|e| MetricsError::Encoding(e.to_string()))?;
        String::from_utf8(buf).map_err(|e| MetricsError::Encoding(e.to_string()))
    }
}

#[cfg(test)]
#[path = "metrics_tests.rs"]
mod tests;
