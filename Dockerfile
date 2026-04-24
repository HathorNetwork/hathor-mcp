# Multi-stage build for hathor-mcp
FROM rust:1-bookworm AS builder

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

# In a container, listening on loopback would make the server unreachable from
# outside the container. We bind 0.0.0.0 explicitly here — the default in the
# binary is 127.0.0.1, which is what we want for locally-run installs.
# Auth (HATHOR_MCP_TOKEN env or --auth-token) should always be set alongside
# this; the binary will refuse to run without auth on a non-loopback bind
# unless --no-auth is passed deliberately.
ENTRYPOINT ["hathor-mcp", "--bind", "0.0.0.0"]
