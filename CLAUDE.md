# OpenApiGateway

High-performance, OpenAPI-native API gateway. Envoy data plane + Go gateway-manager control plane.

## Packages

| Crate | Description | LOC Limit |
|-------|-------------|-----------|
| `controlplane/` | Config management, admin API, spec ingestion | 400 lines/file |
| `dataplane/` | HTTP proxy, traffic handling | 400 lines/file |
| `shared/` | Common types, errors, utilities | 300 lines/file |

## Commands

```bash
cargo fmt --all -- --check     # formatting
cargo clippy --workspace --all-targets -- -D warnings  # lints
cargo test --workspace         # all tests
cargo build --workspace --release  # release build
make check                     # fmt + clippy + test combined
bash tools/check-loc.sh        # Rust LOC enforcement
```

## Rules

- **LOC limits enforced in CI with ZERO exceptions** -- no allowlists
- Read `AGENTS.md` before modifying any crate
- Never push to `develop` or `main` directly -- feature branch + PR only
- No `.unwrap()` in library or production code -- use proper error handling
- All public APIs must have doc comments
- Prefer `thiserror` for error types, `anyhow` only in binaries

## Architecture

- `docs/architecture/API_GATEWAY_DESIGN.md` -- full system design
- `docs/decisions/V1_IMPLEMENTATION_DECISIONS.md` -- frozen v1 choices
- `specs/` -- 6 detailed specification documents (config, auth, rate-limiting, observability, admin-api, base-case)
- Control plane / data plane split: Envoy handles HTTP; gateway-manager handles config, auth, rate-limiting

## Specs Quick Reference

| Spec | Path | Key Decisions |
|------|------|---------------|
| Base Case | `specs/base-case-implementation-spec.md` | Request lifecycle, failure modes |
| Config | `specs/config-schema.md` | YAML schema, validation rules |
| Auth | `specs/auth.md` | JWT RS256 only, JWKS caching |
| Rate Limiting | `specs/rate-limiting.md` | Token bucket, Redis + Lua |
| Observability | `specs/observability.md` | Prometheus, JSON logs, OTLP |
| Admin API | `specs/admin-api.md` | /healthz, /readyz, /config/* |

## Before Starting a Task

1. Which crate and module?
2. Read `AGENTS.md` (root and any inner `AGENTS.md` in the crate)
3. Read the relevant spec in `specs/`
4. `cargo check` to verify starting state
5. Stay under LOC limits -- split if approaching 90%

## Token Optimization Rules

Agents working on this codebase must minimize token usage:

- **Lazy-load context**: Read only files relevant to the current task, never the entire codebase
- **Task-specific profiles**: Each subagent gets only the files and specs it needs
- **Plan-first**: Break large features into explicit task lists before writing code
- **First-pass correctness**: Include I/O examples and pre/post conditions in prompts
- **No blind retries**: On error, include the exact error message in the fix prompt
- **Selective retrieval**: Read specific functions, not entire files, when possible
- **Summarize over replay**: Reference prior decisions by summary, not full re-read
- **Stable prefix**: Keep system instructions and shared context at the top of prompts
- **No prompt bloat**: Cut natural-language filler; feed code snippets and constraints only
