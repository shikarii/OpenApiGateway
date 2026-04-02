# ADR 0004: HTTP ext_authz over gRPC

## Status

Accepted

## Context

Envoy's `ext_authz` filter supports two transport modes for calling an
external authorization service:

1. **gRPC** -- requires a protobuf service definition, tonic server, and
   build-time proto compilation.
2. **HTTP** -- Envoy sends a plain HTTP request with the original request
   headers and path; the service responds with 200 (allow) or 4xx/5xx (deny).

The gateway-manager already uses axum for the admin API. Adding a second
axum server on a different port is trivial. Adding a tonic gRPC server
would require protobuf definitions, `prost-build` compilation, and a
separate service implementation despite tonic being present in
`Cargo.toml`.

## Decision

Use Envoy HTTP ext_authz mode with a second axum server on a configurable
port (default `0.0.0.0:10003`).

Envoy sends the original request method, path, and allowed headers
(authorization, host, x-forwarded-*) to the ext_authz service. The
service responds with injected headers (x-auth-sub, x-ratelimit-remaining,
etc.) that Envoy forwards to the upstream.

## Consequences

- No new dependencies -- reuses axum.
- Debugging is simpler: `curl` can exercise the ext_authz endpoint.
- Slight overhead vs binary gRPC, but negligible for auth/rate-limit
  checks where the bottleneck is Redis/JWKS latency.
- If gRPC becomes necessary later (e.g., for structured CheckResponse
  metadata), this ADR can be superseded.
