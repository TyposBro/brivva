# Multi-stage build for server-rs (workspace member)
FROM rust:1.85-slim AS builder

WORKDIR /app

# Copy workspace root
COPY Cargo.toml Cargo.lock ./
COPY server-rs/Cargo.toml server-rs/Cargo.toml

# Create dummy src to cache deps
RUN mkdir -p server-rs/src && echo "fn main() {}" > server-rs/src/main.rs
RUN cargo build --release --manifest-path server-rs/Cargo.toml || true

# Copy real source and rebuild
COPY server-rs/src server-rs/src
RUN cargo build --release --manifest-path server-rs/Cargo.toml

# Runtime stage
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/server-rs /usr/local/bin/server-rs

EXPOSE 3000

CMD ["server-rs"]
