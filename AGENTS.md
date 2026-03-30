# AGENTS.md

Repository operating rules for coding agents.

## 0. Prime Directive

- You may generate code quickly.
- You may not change architecture without explicit approval.
- If a requested change violates repository contracts, stop and ask for clarification.
- Prefer correctness and boundary integrity over speed.
- NEVER use emojis in code, commit messages, documentation, or any repository artifacts.

## 1. Required Repository Contracts

These artifacts must remain present:

- `AGENTS.md`
- `CLAUDE.md`
- `CONTRIBUTING.md`
- `.github/pull_request_template.md`

Do not introduce or modify licensing terms without explicit maintainer approval.

### Architecture Decision Records

Write an ADR under `docs/adr/` whenever a change involves any of the following:

- New crate, module, or public API surface
- A technology choice (new dependency, transport, storage)
- A performance or scaling constraint that shapes code structure
- Reversal or significant modification of a prior decision

Number sequentially (`0001-short-slug.md`, `0002-short-slug.md`, ...).
Set **Status** to one of: `Proposed`, `Accepted`, `Superseded by ADR NNNN`, or `Rejected`.

## 1.1 Nested Agent Instructions Precedence

- Always discover and read any closer `AGENT.md` or `AGENTS.md` in the directory
  subtree you are changing.
- Treat the nearest inner agent-instructions file as authoritative for that subtree.
- Apply root `AGENTS.md` rules as defaults; let inner rules refine or tighten behavior.
- If instructions conflict and cannot be safely reconciled, stop and ask the maintainer.

## 2. Module Ownership and Boundaries

### Crate layout

```
controlplane/    Config management, admin API, spec ingestion.
                 src/main.rs      -- Bootstrap + runtime wiring ONLY.
                 src/config/      -- Config parsing, validation, Envoy generation.
                 src/admin/       -- Admin API endpoints (/healthz, /readyz, /config/*).
                 src/auth/        -- JWT/JWKS validation.
                 src/ratelimit/   -- Token bucket (Redis + in-memory fallback).
                 src/observability/ -- Metrics, access logs, tracing.

dataplane/       High-performance HTTP proxy (traffic handling).
                 src/main.rs      -- Bootstrap + runtime wiring ONLY.

shared/          Common types and utilities shared across crates.
                 src/lib.rs       -- Module re-exports ONLY.
                 src/error.rs     -- GatewayError enum.
                 src/types.rs     -- Shared domain types.
```

### Dependency rules

- `shared/` must not import from `controlplane/` or `dataplane/`.
- `controlplane/` and `dataplane/` may import from `shared/`.
- `controlplane/` and `dataplane/` must not import from each other.
- Keep I/O (network, filesystem) behind explicit boundary modules.
- Pure logic modules must not import I/O modules.

## 3. File Size Limits

### Hard limits

| File type | Max lines | Notes |
|-----------|-----------|-------|
| `.rs` implementation | **400** | Split into submodules if approaching |
| `main.rs` | **150** | Bootstrap + runtime setup only |
| `lib.rs` | **100** | Module declarations and re-exports only |
| `mod.rs` | **100** | Submodule declarations and re-exports only |
| `build.rs` | **200** | Build scripts (protobuf generation, etc.) |

### Soft guidelines

- Functions over **60 lines** should be extracted into a named helper or separate file.
- If a `.rs` file exceeds **300 lines**, consider whether it has multiple responsibilities
  that could be split.
- Data tables (route specs, error mappings) may push files toward the limit -- this is
  acceptable if the file has a single responsibility.

### Current violations

None. All files are within limits.

## 4. Quality and Enforcement

### Rust-specific rules

- **No `.unwrap()` or `.expect()` in library code** -- use `?` operator with proper error types.
  `.unwrap()` is permitted only in tests and `main.rs` bootstrap.
- **No `unsafe` without an accompanying `// SAFETY:` comment** explaining why it is sound.
- **No `#[allow(clippy::*)]` without a `// Reason:` comment** explaining why the lint is wrong.
- **No `todo!()` or `unimplemented!()` in merged code** -- use `TODO(#issue):` comments instead.
- **No `pub` on internal types** -- minimize public API surface. Use `pub(crate)` or `pub(super)`.
- **Prefer `impl Trait` over `dyn Trait`** unless dynamic dispatch is genuinely needed.

### Testing requirements

Every new feature that contains non-trivial logic **must** be accompanied by unit tests
in the same PR. This applies in particular to:

- Config parsing and validation logic
- JWT claim validation and JWKS caching
- Rate-limiting token bucket arithmetic
- Error handling and fallback behavior
- Any function with a defined mathematical invariant or boundary condition

Test files live alongside the code they cover using `#[cfg(test)] mod tests { ... }` blocks
for unit tests. Integration tests go in `tests/`.

### Before every push and before opening a PR

```
cargo fmt --all -- --check     # must pass with zero errors
cargo clippy --workspace --all-targets -- -D warnings  # must pass
cargo test --workspace         # must pass with zero failures
bash tools/check-loc.sh        # must pass with zero violations
```

### Adversarial self-review

For every implementation task, run an explicit adversarial pass before finalizing:

- Try to break it with malformed inputs and boundary values.
- Check for regressions in config handling, auth flows, and rate-limit state.
- Check for silent failure paths and misleading success states.
- Verify all error responses match the spec exactly.

### PR bug sweep

For every pull request, perform a bug-focused review of all changed files:

- Inspect each changed file directly (do not sample a subset).
- Record findings by severity; if no findings exist, state that explicitly.
- Call out residual risk and test gaps.

## 5. Entropy Prevention

- No `utils` module names for new code -- name by responsibility.
- No `TODO` without issue reference (example: `TODO(#12): ...`).
- No new public type without doc comments.
- No `clone()` in hot paths without justification.

### Rust module size guidelines

- Target: 100-250 LOC
- Soft limit: 300 LOC
- Hard limit: 400 LOC
- If a file exceeds 300 LOC, split by responsibility.

## 6. Completion Rule

After each completed prompt:

1. Implement
2. Validate -- `cargo fmt`, `cargo clippy`, `cargo test`, and `bash tools/check-loc.sh` must pass
3. Commit on a feature branch (never directly to `develop` or `master`)
4. Push and open PR to `develop` with a linked issue URL
5. Merge via PR after checks pass

## 7. Branch and PR Discipline

- Always branch from latest `develop`.
- Never commit directly to `master`, `main`, or `develop`.
- Keep changes isolated to one human-readable feature branch per task.
- Every PR must include at least one explicit issue reference using a full URL.
  Example: `Closes https://github.com/shikarii/OpenApiGateway/issues/1`
- Do not close issues until the PR is merged.

## 8. Human-Facing Wording for Issues and PRs

- Use human, specific language over boilerplate.
- Prefer concrete sentences about intent and impact.
- Link issues with full URLs, not just `#123`.
- State tradeoffs and test coverage explicitly.
- Never write literal `\n` sequences in GitHub issue/PR text. Use real line breaks
  and `--body-file` when scripting `gh` commands.

## 9. Software Design Principles

**Single Responsibility** -- Every module has exactly one reason to change. Separate
config parsing, validation, auth, rate-limiting, and observability into distinct modules.

**Dependency Inversion** -- Pure business logic (config validation, token bucket math,
JWT claim checking) must not import I/O modules. I/O modules depend on logic; logic
does not depend on I/O.

**Fail-Safe Defaults** -- Malformed configs are rejected atomically. Redis failures
trigger degraded mode. JWKS staleness triggers 503. Silent state corruption is not
acceptable.

**Explicit Error Handling** -- Every error path returns a typed error with context.
No `.unwrap()` in production code. Use `thiserror` for library errors, `anyhow` only
in binary entry points.

**Observe Everything** -- Log config reloads, auth failures, rate-limit decisions,
and upstream errors. Debugging a distributed gateway requires visibility at every stage.

**Stateless Hot Path** -- JWT validation and rate-limit checks must require zero
database lookups on the critical path (CPU-only for JWT, atomic Lua for Redis).

## 10. Token Optimization for LLM-Assisted Development

Agents working on this codebase must minimize token consumption:

- **Lazy-load context on demand** -- Do not read the entire codebase. Retrieve only
  files relevant to the current task. Use grep/glob to find targets first.
- **Task-specific context** -- Each subagent receives only the spec and source files
  it needs. Do not include unrelated specs or crates.
- **Plan-first, then implement** -- Break large features into an explicit task list
  before writing code. This reduces ambiguity and iteration loops.
- **First-pass correctness** -- Include I/O examples, pre/post conditions, and
  constraints in prompts. Specify input/output formats explicitly to avoid rework.
- **No blind retries** -- On compile or test failure, include the exact error message
  in the fix prompt. Do not regenerate from scratch.
- **Selective code retrieval** -- Read specific functions or modules, not entire files,
  when a targeted read suffices.
- **Summarize over replay** -- Reference prior decisions by short summary, not by
  re-reading full conversation history.
- **Embed static analysis** -- Run `cargo check` or `cargo clippy` before prompting
  for fixes, and include the output to guide corrections.
- **Stable prompt prefix** -- Keep system instructions and shared context at the top
  of prompts to maximize prefix caching.
- **No prompt bloat** -- Cut filler text. Feed code snippets and constraints only.
  If context is long, summarize it or retrieve selectively.
