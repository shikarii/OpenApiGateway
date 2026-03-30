# Design

## Architecture Overview

OpenApiGateway uses a **control plane / data plane** split architecture:

```
                  ┌─────────────────┐
                  │  Control Plane  │
                  │  (admin API,    │
                  │   spec mgmt)    │
                  └────────┬────────┘
                           │ gRPC (proto/)
                  ┌────────▼────────┐
                  │   Data Plane    │
                  │  (proxy, route, │
                  │   validate)     │
                  └─────────────────┘
```

### Data Plane (`dataplane/`)

The hot path. Handles all inbound HTTP traffic:

- **Listener** — accepts TCP connections, TLS termination
- **Router** — matches requests to OpenAPI paths/operations
- **Validator** — enforces request/response schemas from the spec
- **Upstream** — forwards validated requests to backend services
- **Filters** — pluggable middleware (rate limiting, auth, transforms)

Design constraints:
- Zero-allocation hot path where possible
- Async I/O via Tokio
- No dynamic dispatch in the request pipeline

### Control Plane (`controlplane/`)

The management layer. Runs separately from traffic:

- **Spec Ingestion** — parse and compile OpenAPI 3.x specs
- **Route Table** — build optimized routing structures, push to data plane
- **Admin API** — REST/gRPC endpoints for configuration
- **Health / Metrics** — readiness probes, Prometheus metrics

### Shared (`shared/`)

Common types used by both planes:

- Error types and result aliases
- OpenAPI model types
- Configuration structures
- Telemetry helpers

### Proto (`proto/`)

Protobuf service definitions for control-to-data-plane communication:

- `RouteSync` — push updated route tables
- `ConfigSync` — push configuration changes
- `HealthCheck` — liveness/readiness between planes

## Key Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Language | Rust | Performance, safety, async ecosystem |
| Async runtime | Tokio | Industry standard, mature |
| HTTP framework | Hyper | Low-level control, zero-copy |
| Serialization | serde + serde_json | Ecosystem standard |
| Schema validation | Custom | OpenAPI-specific optimizations |
| Inter-plane comms | gRPC (tonic) | Typed, efficient, streaming |
| Config format | TOML | Rust ecosystem convention |
