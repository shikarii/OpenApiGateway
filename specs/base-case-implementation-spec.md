# Base Case Implementation Specification

This document specifies the canonical, reference implementation of the API Gateway v1.

## Executive Summary

The v1 gateway is a local-first, production-grade API Gateway that serves as the critical ingress boundary for microservices architectures on 1-5 node clusters. It provides:

- **Request routing** by hostname and longest-prefix path match
- **Stateless authentication** via JWT + JWKS
- **Rate limiting** via Redis + Token Bucket algorithm
- **Observability** via Prometheus metrics and JSON access logs
- **Graceful failure** with degraded-mode fallbacks

## Request Lifecycle

Every request follows this deterministic pipeline:

1. **TLS Termination** — Envoy accepts TCP, negotiates TLS
2. **HTTP Parsing** — Raw bytes → HTTP representation with header normalization
3. **Route Match** — Hostname exact match, longest path prefix wins
4. **Method Check** — Verify HTTP method is allowed for route
5. **Authentication** — If route requires auth, validate JWT (see [auth.md](auth.md))
6. **Rate Limit Check** — If route has rate limit policy, check Redis token bucket (see [rate-limiting.md](rate-limiting.md))
7. **Upstream Selection** — Pick healthy endpoint from route's upstream service
8. **Proxy** — Forward request, stream response
9. **Observability** — Emit access log and metrics (see [observability.md](observability.md))

## Key Design Decisions

### Why Envoy as Data Plane?

- Hardened, production-tested proxy (avoids building TLS, HTTP/2, QUIC from scratch)
- Can eventually evolve to xDS without rearchitecture
- Stable, well-understood failure modes
- Strong operational tooling

### Why File-Based Config, Not xDS?

- Eliminates control-plane complexity for v1
- Still provides atomic config updates (validate, write, restart or reload safely)
- Trivial to layer xDS on top later (gateway-manager → xDS server)
- Agents have clear success criteria

### Why JWT, Not Session Cookies?

- Completely stateless validation (CPU-only, no database lookups)
- Scales from 1 node to hyperscale without architectural change
- JWKS caching eliminates hard dependency on external IdP
- Clear failure modes (expired token = 401, untrusted signature = 401)

### Why Redis + Lua for Rate Limiting?

- Atomic, race-condition-free token bucket logic via Lua atomicity
- Single point of truth ensures fair distribution across replicas
- Graceful degradation: if Redis fails, fall back to local in-memory bucket
- Scales from 1 Redis instance to sharded cluster without code changes

## Config Model

The canonical config for v1 is `configs/gateway.yaml`. See [config-schema.md](config-schema.md) for the full schema, validation rules, and examples.

## Admin API

The gateway exposes a lightweight admin API on `:9090` (separate from the gateway port). See [admin-api.md](admin-api.md) for the full specification.

## Deployment Model

For v1, the standard deployment is:

```yaml
services:
  redis:
    image: redis:7
    ports: ["6379:6379"]

  echo-backend:  # placeholder upstream 
    build: ./services/echo-backend
    ports: ["8081:8081"]

  gateway-manager:
    build: ./services/gateway-manager
    ports:
      - "8080:8080"     # Gateway port
      - "9090:9090"     # Admin port
    volumes:
      - ./configs:/app/configs

  envoy:
    image: envoyproxy/envoy:v1.31-latest
    ports:
      - "80:10000"      # HTTP egress from Envoy
      - "443:10001"     # HTTPS egress (optional)
    volumes:
      - ./configs/envoy.yaml:/etc/envoy/envoy.yaml
```

See [../examples/docker-compose/](../examples/docker-compose/) and [../../deployments/docker-compose/](../../deployments/docker-compose/) for complete examples.

## Observability Contract

The gateway emits:

- **Access logs** (JSON, stdout): Every request
- **Prometheus metrics** (HTTP /metrics): Scraped every 15-30 seconds
- **Distributed traces** (optional OTLP): If `tracing.enabled: true` in config

See [observability.md](observability.md) for detailed schemas.

## Failure Modes & Mitigations

| Failure | Behavior | Mitigation |
| :---- | :---- | :---- |
| Redis unavailable | Rate limiter times out | Fall back to local in-memory token bucket, emit `x-rate-limit-mode: degraded-local` |
| JWKS fetch fails | Use cached JWKS | If cache age > 10 * cache_ttl_seconds, deny protected routes with 503 |
| Upstream unavailable | No healthy endpoints | Return 503 with `{"error":"upstream_unavailable"}` |
| Gateway overloaded | > 1000 concurrent requests | Return 503 with `x-gateway-overloaded: true` and `retry-after: 1` |
| Config invalid | Reject load | Keep last known-good config, expose error via admin `/config/status` |

## Testing Strategy

V1 requires these test categories:

1. **Config validation tests** — Invalid schemas, missing fields, type mismatches
2. **JWT tests** — Valid/expired/malformed/wrong-signature tokens
3. **Rate limit tests** — Token bucket behavior, Redis failures, local fallback
4. **Route matching tests** — Host/path/method ordering and precedence
5. **Integration tests** — End-to-end request flow with mock backends

See [../../tests/](../../tests/) for the test structure.

## Success Criteria

A correct v1 implementation:

- [ ] Routes requests by host + path according to longest-prefix-match order
- [ ] Validates JWTs using JWKS and enforces required scopes
- [ ] Rate-limits requests via Redis token bucket with atomic Lua execution
- [ ] Falls back to in-memory token bucket if Redis is unavailable
- [ ] Emits structured JSON access logs with all required fields
- [ ] Exposes Prometheus metrics with bounded cardinality (no raw paths/user IDs)
- [ ] Handles auth/JWKS/rate-limit/upstream failures gracefully with appropriate status codes
- [ ] Reloads config atomically without dropping in-flight requests
- [ ] Passes all config validation, unit, and integration tests
