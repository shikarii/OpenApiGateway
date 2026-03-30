use opentelemetry::trace::TracerProvider;
use opentelemetry_otlp::WithExportConfig;
use shared::config_types::TracingConfig;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

/// Errors during tracing initialization.
#[derive(Debug, thiserror::Error)]
pub(crate) enum TracingInitError {
    #[error("failed to initialize OTLP exporter: {0}")]
    OtlpInit(String),
    #[error("failed to set global subscriber: {0}")]
    SetGlobal(String),
}

/// Initialize the tracing subscriber.
///
/// When `tracing_config.enabled` is true, configures an OpenTelemetry
/// OTLP exporter alongside the default fmt subscriber. When disabled,
/// uses the standard fmt subscriber with env filter only.
///
/// Must be called exactly once, before any tracing macros are used.
pub(crate) fn init_tracing(tracing_config: &TracingConfig) -> Result<(), TracingInitError> {
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let fmt_layer = tracing_subscriber::fmt::layer();

    if tracing_config.enabled {
        let sampler = opentelemetry_sdk::trace::Sampler::TraceIdRatioBased(
            tracing_config.sample_rate.clamp(0.0, 1.0),
        );

        let exporter = opentelemetry_otlp::SpanExporter::builder()
            .with_tonic()
            .with_endpoint(&tracing_config.otlp_endpoint)
            .build()
            .map_err(|e: opentelemetry::trace::TraceError| {
                TracingInitError::OtlpInit(e.to_string())
            })?;

        let tracer_provider = opentelemetry_sdk::trace::SdkTracerProvider::builder()
            .with_sampler(sampler)
            .with_batch_exporter(exporter)
            .build();

        let tracer = tracer_provider.tracer("api-gateway");

        // Store provider for shutdown access and register globally.
        let _ = TRACER_PROVIDER.set(tracer_provider.clone());
        opentelemetry::global::set_tracer_provider(tracer_provider);

        let otel_layer = tracing_opentelemetry::layer().with_tracer(tracer);

        tracing_subscriber::registry()
            .with(env_filter)
            .with(fmt_layer)
            .with(otel_layer)
            .try_init()
            .map_err(|e: tracing_subscriber::util::TryInitError| {
                TracingInitError::SetGlobal(e.to_string())
            })?;
    } else {
        tracing_subscriber::registry()
            .with(env_filter)
            .with(fmt_layer)
            .try_init()
            .map_err(|e: tracing_subscriber::util::TryInitError| {
                TracingInitError::SetGlobal(e.to_string())
            })?;
    }

    Ok(())
}

/// Shut down the OpenTelemetry tracer provider, flushing pending spans.
///
/// The `SdkTracerProvider` is stored in a module-level `OnceLock` so we can
/// call `shutdown()` on it during graceful termination.
pub(crate) fn shutdown_tracing() {
    if let Some(provider) = TRACER_PROVIDER.get() {
        let _ = provider.shutdown();
    }
}

/// Holds the `SdkTracerProvider` for shutdown access.
static TRACER_PROVIDER: std::sync::OnceLock<opentelemetry_sdk::trace::SdkTracerProvider> =
    std::sync::OnceLock::new();

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display_messages() {
        let e = TracingInitError::OtlpInit("connection refused".into());
        assert!(e.to_string().contains("connection refused"));

        let e = TracingInitError::SetGlobal("already set".into());
        assert!(e.to_string().contains("already set"));
    }

    // Note: init_tracing can only be called once per process.
    // The disabled path is tested implicitly by main.rs integration.
    // The enabled path requires a running OTLP collector.
}
