# ADR 0001: JWT Parsing and JWKS HTTP Client Crate Selection

Status: Accepted

## Context

Issue #11 requires RS256 JWT signature verification and JWKS endpoint fetching.
Neither capability exists in the current workspace dependencies.

## Decision

- `jsonwebtoken = "9"` for JWT decode/verify.
- `reqwest = { version = "0.12", features = ["json", "rustls-tls"] }` for JWKS HTTP.

Both added only to `controlplane/Cargo.toml`.

## Rationale

### jsonwebtoken

- RS256 via `DecodingKey::from_rsa_components(n, e)`.
- Configurable claim validation (exp, nbf, iss, aud).
- Pure Rust RSA; no OpenSSL.
- Alternative considered: `josekit` -- heavier dependency graph.

### reqwest

- Tokio-native async HTTP with connection pooling.
- `rustls-tls` avoids OpenSSL for hermetic Docker builds.
- Built-in JSON deserialization and redirect following.
- Alternative considered: raw `hyper` -- already in workspace but requires
  boilerplate for redirects, JSON parsing, and TLS configuration.

## Consequences

- Binary size increases ~1.5 MB (rustls + ring).
- No OpenSSL system dependency required.
- Both crates scoped to controlplane only; shared and dataplane unaffected.
