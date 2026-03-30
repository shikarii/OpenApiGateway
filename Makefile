.PHONY: build test check fmt clippy clean run-dataplane run-controlplane

build:
	cargo build --workspace

test:
	cargo test --workspace

check: fmt clippy test

fmt:
	cargo fmt --all -- --check

clippy:
	cargo clippy --workspace --all-targets -- -D warnings

clean:
	cargo clean

run-dataplane:
	cargo run -p dataplane

run-controlplane:
	cargo run -p controlplane
