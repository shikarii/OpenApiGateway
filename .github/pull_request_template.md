## Summary

Describe what changed and why it matters.

## Linked Issue

Use a closing keyword with a full URL (required):

- Closes https://github.com/shikarii/OpenApiGateway/issues/<number>

## Base Branch

- [ ] This PR targets `develop`

## Validation

List exact commands and outcomes.

- [ ] `cargo fmt --all -- --check` passed
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` passed
- [ ] `cargo test --workspace` passed
- [ ] `bash tools/check-loc.sh` passed (all files within LOC limits)

## Risks and Tradeoffs

Call out any impact to existing behavior, compatibility concerns, and rollback path.

## Adversarial Review

Describe edge cases you tested and what could still fail.
