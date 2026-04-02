FROM rust:bookworm AS builder
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY shared/ shared/
COPY controlplane/ controlplane/
COPY dataplane/ dataplane/
RUN cargo build --release -p controlplane

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates wget \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /build/target/release/controlplane /usr/local/bin/gateway-manager
ENTRYPOINT ["gateway-manager"]
