# V1 Implementation Decisions & Frozen Choices

## Overview

For v1, the agent must implement a local-first API gateway with these non-negotiable choices:

- **Data plane:** Envoy
- **Custom code language:** Go
- **Initial deployment model:** Docker Compose on 1 to 5 machines
- **Config model:** file-based YAML, hot-reloaded by replacing files and restarting the container
- **Service discovery:** static upstream lists in config for v1
- **Auth:** JWT validation using JWKS
- **Rate limiting:** Redis + Lua token bucket
- **Observability:** Prometheus metrics, JSON access logs, optional OTEL trace export
- **No control plane** in v1
- **No Consul** in v1
- **No xDS** in v1
- **No hierarchical quota system** in v1
- **No tail-based sampling** in v1

This matches the MVP direction in your architecture docs and keeps the base case aligned with the local-first recommendation instead of jumping into hyperscale machinery too early.

## V1 Request Path

The gateway must support this minimal but complete request path:

1. Accept HTTP request
2. Match route by host + path prefix
3. Optionally require JWT auth
4. Optionally apply rate limit policy
5. Proxy to configured upstream pool
6. Emit structured access log
7. Expose Prometheus metrics
8. Return upstream response

This stays faithful to the request lifecycle and edge responsibilities in the architecture docs, but trims away control-plane complexity.

## Repository Structure

Use this exact repo shape:

```
api-gateway/
  README.md
  Makefile
  .env.example

  configs/
    gateway.yaml
    envoy.yaml
    jwks-dev.json

  infra/
    docker-compose.yml
    prometheus.yml
    redis.conf

  services/
    gateway-manager/
      cmd/gateway-manager/main.go
      internal/config/config.go
      internal/jwks/jwks.go
      internal/ratelimit/redis_lua.go
      internal/admin/admin.go
      internal/types/types.go
      internal/validation/validation.go

    echo-backend/
      cmd/echo-backend/main.go

  envoy/
    envoy.yaml.tmpl
    lua/
      ratelimit.lua

  scripts/
    gen-jwt-dev.py
    curl-smoke.sh
    load-test.sh

  tests/
    config_validation_test.go
    jwks_test.go
    ratelimit_test.go
    integration_test.go
```

### Role of Each Component

**gateway-manager** exists because the base case still needs some glue logic that agents otherwise invent inconsistently. It owns:

- Gateway config parsing
- Config validation
- JWKS fetch and cache
- Redis rate limit helper behavior
- Admin endpoints for health and reload status
- Envoy config generation from your canonical YAML

**Envoy** remains the data plane because the architecture docs already converge on reusing a hardened proxy rather than building parsing/TLS/proxying from scratch.

## Rationale

1. **MVP Focus:** V1 is intentionally minimal. It establishes the foundational patterns (JWT, rate limit, observability) without premature scaling machinery.
2. **Future Evolution:** This structure cleanly evolves:
   - Consul + gossip discovery can layer in later
   - xDS servers can replace Envoy YAML templating
   - Control plane can be introduced without rearchitecturing
   - Hierarchical quota can migrate from single Redis
3. **Implementability:** Each component is bounded and testable. Agents know exactly what success looks like.
