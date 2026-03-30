# ADR 0003: Observability Crate Selection

Status: Accepted

## Context

Issue #13 requires Prometheus metrics at `/metrics`, structured JSON access logs,
and optional OTLP trace export. We need crates for metrics collection/encoding,
timestamp formatting, and OpenTelemetry integration.

## Decision

### Prometheus metrics
`prometheus = { version = "0.13", default-features = false }`

### Timestamps
`chrono = { version = "0.4", features = ["clock", "serde"], default-features = false }`

### Distributed tracing (optional OTLP export)
```
opentelemetry = "0.28"
opentelemetry_sdk = { version = "0.28", features = ["rt-tokio"] }
opentelemetry-otlp = { version = "0.28", features = ["tonic"] }
tracing-opentelemetry = "0.29"
```

## Rationale

- `prometheus`: Direct text exposition output via `TextEncoder`. Battle-tested
  (TiKV, Linkerd2-proxy). No facade abstraction needed — the gateway owns its
  metrics surface. `default-features = false` avoids protobuf dependency.
- `chrono`: RFC 3339 millisecond-precision timestamps for access logs. Standard
  Rust datetime library; `default-features = false` avoids `oldtime`/`std` bloat.
- OpenTelemetry stack: `tracing-opentelemetry` bridges the existing `tracing`
  subscriber to OTLP. `tonic` feature reuses workspace gRPC dependency.
- Alternative considered: `metrics` + `metrics-exporter-prometheus` — more
  abstract but adds indirection with no benefit for a single-backend gateway.

## Consequences

- Scoped to controlplane only; shared and dataplane unaffected.
- OpenTelemetry adds ~2 MB to binary. Acceptable for optional trace export.
- If opentelemetry version conflicts arise with workspace tonic, OTLP export
  defers to a follow-up; metrics and logs are independent.
