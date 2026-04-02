.PHONY: build test check fmt clippy clean loc run-dataplane run-controlplane \
       docker-build docker-run docker-stop

build:
	cargo build --workspace

release:
	cargo build --workspace --release

test:
	cargo test --workspace

check: fmt clippy test loc

fmt:
	cargo fmt --all -- --check

clippy:
	cargo clippy --workspace --all-targets -- -D warnings

loc:
	bash tools/check-loc.sh

clean:
	cargo clean

run-dataplane:
	cargo run -p dataplane

run-controlplane:
	cargo run -p controlplane

# Docker targets
docker-build:
	docker build -t gateway-manager:latest -f services/gateway-manager/Dockerfile .

docker-run:
	docker compose -f examples/single-node/docker-compose.yml up -d --build

docker-stop:
	docker compose -f examples/single-node/docker-compose.yml down -v
