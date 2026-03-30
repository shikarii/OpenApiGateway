# OpenApiGateway

OpenAPI-native API gateway. Control plane / data plane split. Rust.

## Packages

| Crate | Description |
|-------|-------------|
| `dataplane/` | High-performance HTTP proxy — routing, validation, forwarding |
| `controlplane/` | Admin API, spec ingestion, config management |
| `shared/` | Common types, errors, config, telemetry |
| `proto/` | Protobuf/gRPC service definitions |

## Commands

```bash
make build          # build all crates
make test           # run all tests
make check          # fmt + clippy + test
make run-dataplane  # run data plane locally
make run-controlplane  # run control plane locally
```

## Rules

- **Never push to `develop` or `main` directly** — feature branch + PR only
- All PRs must pass CI: fmt, clippy, test, build
- Keep crates focused — shared logic goes in `shared/`
- Prefer explicit error types over `anyhow` in library code
- Public APIs get doc comments
- Proto changes require regenerating Rust bindings

## Before Starting a Task

1. Which crate does this touch?
2. Read relevant docs in `docs/`
3. `make check` to verify starting state
4. Write tests for new functionality
