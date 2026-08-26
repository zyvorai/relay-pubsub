FROM rust:1-bookworm AS builder
WORKDIR /src
COPY Cargo.toml build.rs ./
COPY proto ./proto
COPY src ./src
RUN cargo build --release --locked || cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates wget && rm -rf /var/lib/apt/lists/*
COPY --from=builder /src/target/release/relay-pubsub /usr/local/bin/relay-pubsub
EXPOSE 50051 8080
USER 65532:65532
ENTRYPOINT ["/usr/local/bin/relay-pubsub"]
