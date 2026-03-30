# Roadmap

## Phase 1 — Foundation

- [ ] Workspace scaffold (Cargo workspace, CI, lints)
- [ ] Data plane TCP listener + basic HTTP proxy
- [ ] OpenAPI 3.0 spec parser
- [ ] Path-based routing from spec
- [ ] Control plane admin API skeleton
- [ ] gRPC route sync between planes

## Phase 2 — Validation

- [ ] Request validation (path params, query params, headers, body)
- [ ] Response validation (optional, for debugging)
- [ ] JSON Schema validation engine
- [ ] Error response formatting (RFC 7807)

## Phase 3 — Production Readiness

- [ ] TLS termination
- [ ] Graceful shutdown and hot reload
- [ ] Prometheus metrics
- [ ] Structured logging (tracing)
- [ ] Health check endpoints
- [ ] Docker and Kubernetes deployment manifests

## Phase 4 — Advanced Features

- [ ] Rate limiting (token bucket)
- [ ] Authentication middleware (JWT, API key)
- [ ] Request/response transformation
- [ ] OpenAPI 3.1 support
- [ ] WebSocket proxying
- [ ] Plugin system for custom filters
