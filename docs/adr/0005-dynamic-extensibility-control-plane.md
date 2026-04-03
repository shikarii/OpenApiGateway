# ADR 0005: Dynamic Extensibility and Control Plane

## Status

Accepted

## Context

The gateway roadmap now requires three related capabilities:

1. A safe extension model so new gateway behavior can be added without
   hardcoding every feature in Rust.
2. A native Envoy control-plane protocol for zero-downtime config updates.
3. Bidirectional request/response processing so future response validation
   can inspect downstream and upstream traffic.

These introduce new dependencies (`mlua`, `jsonschema`, `dashmap`,
`prost-types`, `tonic-build`, `protoc-bin-vendored`) and two new transport
layers (`xDS` ADS and Envoy `ext_proc`).

## Decision

Adopt a combined extensibility and control-plane foundation with these choices:

- Use sandboxed LuaJIT via `mlua` for the plugin runtime.
- Keep one thread-local Lua VM per worker thread and rebuild plugin chains
  atomically on config reload.
- Vendor the required Envoy and protobuf definitions in `proto/` and
  generate Rust types at build time with `tonic-build` plus a vendored
  `protoc` binary.
- Implement state-of-the-world ADS first for xDS pushes.
- Run `ext_proc` as a separate tonic streaming service alongside the
  existing HTTP `ext_authz` server.

## Consequences

- The control plane can push LDS/RDS/CDS/EDS snapshots without writing a
  static Envoy file on every change.
- Response-phase processing now has a dedicated gRPC foundation.
- Plugin configuration can be validated before activation, and plugin
  execution remains bounded by sandbox resource limits.
- The codebase gains generated protobuf artifacts and more startup wiring,
  so module boundaries and LOC limits must be enforced carefully.
