# OpenApiGateway

A high-performance, OpenAPI-native API gateway built in Rust.

OpenApiGateway validates, routes, and transforms HTTP traffic using OpenAPI 3.x specifications as the single source of truth. Drop in your spec and the gateway enforces it — no hand-written route config, no schema drift.

## Features

- **Spec-driven routing** — routes derived directly from OpenAPI paths and operations
- **Request/response validation** — schema enforcement at the edge
- **Hot-reload** — update specs without downtime
- **Control plane / data plane split** — manage configuration separately from traffic handling
- **gRPC-native internals** — control plane communicates via protobuf

## Project Structure

```
dataplane/       Rust — high-performance proxy that handles traffic
controlplane/    Rust — configuration management, spec ingestion, admin API
shared/          Rust — common types, utilities, error handling
proto/           Protobuf — gRPC service definitions between planes
docs/            Architecture docs and ADRs
specs/           Example OpenAPI specifications
examples/        Usage examples and quickstarts
deployments/     Docker, Kubernetes, and Compose manifests
scripts/         Dev and CI helper scripts
tests/           Integration and end-to-end tests
tools/           CLI utilities and code generators
```

## Getting Started

### Prerequisites

- Rust 1.78+ (stable)
- Protobuf compiler (`protoc`)

### Build

```bash
make build
```

### Test

```bash
make test
```

### Run

```bash
make run-dataplane
make run-controlplane
```

## Development

See [CONTRIBUTING.md](CONTRIBUTING.md) for development workflow, branch conventions, and CI details.

See [DESIGN.md](DESIGN.md) for architecture and design decisions.

See [ROADMAP.md](ROADMAP.md) for planned milestones.

## License

Licensed under the Apache License, Version 2.0. See [LICENSE](LICENSE) for details.
