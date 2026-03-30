# Contributing to OpenApiGateway

## Development Workflow

1. Fork the repository
2. Create a feature branch from `develop`: `git checkout -b feature/your-feature develop`
3. Make your changes
4. Run `make check` to verify formatting, lints, and tests pass
5. Open a pull request targeting `develop`

## Branch Convention

| Branch | Purpose |
|--------|---------|
| `main` | Stable releases only |
| `develop` | Integration branch — all PRs target here |
| `feature/*` | New features |
| `fix/*` | Bug fixes |

**Never push directly to `develop` or `main`.** All changes go through pull requests.

## CI Requirements

All PRs must pass these checks before merge:

- `cargo fmt --check` — formatting
- `cargo clippy` — lints
- `cargo test` — unit + integration tests
- `cargo build` — release build

## Code Style

- Follow standard Rust conventions (`rustfmt` defaults)
- Keep functions focused and short
- Prefer explicit error handling over `.unwrap()`
- Write doc comments for public APIs

## Commit Messages

Use conventional commits:

```
feat: add OpenAPI 3.1 schema validation
fix: handle empty path parameters
docs: update dataplane architecture section
test: add integration tests for route matching
```

## Reporting Issues

Open an issue with:
- Steps to reproduce
- Expected vs actual behavior
- Rust version and OS

## License

By contributing, you agree that your contributions will be licensed under the Apache License 2.0.
