# Multi-stage build for hathor-mcp
FROM rust:1.83-bookworm AS builder

WORKDIR /app

# Pre-cache deps by copying manifests first
COPY Cargo.toml Cargo.lock ./
RUN mkdir -p src && echo "fn main() {}" > src/main.rs && \
    cargo build --release && \
    rm -rf src target/release/deps/hathor_mcp* target/release/hathor-mcp*

# Real build
COPY src ./src
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && \
    apt-get install -y --no-install-recommends ca-certificates curl && \
    rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/hathor-mcp /usr/local/bin/hathor-mcp

EXPOSE 9876

ENTRYPOINT ["hathor-mcp"]
