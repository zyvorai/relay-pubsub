# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: Apache-2.0
FROM rust:1-bookworm AS builder
RUN apt-get update && apt-get install -y --no-install-recommends cmake && rm -rf /var/lib/apt/lists/*
WORKDIR /src
COPY Cargo.toml build.rs ./
COPY proto ./proto
COPY src ./src
RUN cargo build --release --locked || cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates wget && rm -rf /var/lib/apt/lists/*
COPY --from=builder /src/target/release/relay-pubsub /usr/local/bin/relay-pubsub
# Self-signed TLS cert/key are generated here on first start (see
# PUBSUB_TLS_CERT/PUBSUB_TLS_KEY) — needs to be writable by the non-root user.
RUN mkdir -p /var/lib/relay-pubsub/tls && chown -R 65532:65532 /var/lib/relay-pubsub
EXPOSE 50051 8080
USER 65532:65532
ENTRYPOINT ["/usr/local/bin/relay-pubsub"]
