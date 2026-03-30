# ADR 0002: Redis Client Crate Selection for Rate Limiting

Status: Accepted

## Context

Issue #12 requires atomic token bucket execution on Redis via Lua script,
a multiplexed async connection, configurable command timeouts, and EVALSHA
with EVAL fallback.

## Decision

`redis = { version = "0.27", features = ["tokio-comp", "script"], default-features = false }`

## Rationale

- `redis::Script` provides EVALSHA with automatic EVAL fallback and SHA1 caching.
- `MultiplexedConnection` is Tokio-native and cheaply cloneable.
- `tokio-comp` gives full async/await without a sync thread pool.
- Minimal transitive deps with `default-features = false`.
- Alternative considered: `fred` -- more features (pooling, cluster, sentinel)
  but heavier dependency graph and not needed for single-node v1.
- Alternative considered: `deadpool-redis` -- pool layer unnecessary for a
  single multiplexed connection.

## Consequences

- Binary size increase ~0.8 MB.
- No native-TLS or OpenSSL dependency (default-features disabled).
- Scoped to controlplane only; shared and dataplane unaffected.
