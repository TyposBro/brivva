# Multi-stage build for server-rs (workspace member)
FROM rust:1.85-slim AS builder

WORKDIR /app

# Copy workspace root and stub missing members
COPY Cargo.toml Cargo.lock ./
COPY server-rs/Cargo.toml server-rs/Cargo.toml
RUN mkdir -p frame-extraction/src && \
    echo '[package]\nname = "frame-extraction"\nversion = "0.1.0"\nedition = "2021"' > frame-extraction/Cargo.toml && \
    echo "fn main() {}" > frame-extraction/src/main.rs

# Install SQLite dev libs for sqlx
RUN apt-get update && apt-get install -y --no-install-recommends \
    libsqlite3-dev pkg-config \
    && rm -rf /var/lib/apt/lists/*

# Create dummy src to cache deps
RUN mkdir -p server-rs/src && echo "fn main() {}" > server-rs/src/main.rs
RUN cargo build --release --manifest-path server-rs/Cargo.toml || true

# Copy real source and rebuild (touch to bust cache on dummy main)
COPY server-rs/src server-rs/src
RUN touch server-rs/src/main.rs && cargo build --release --manifest-path server-rs/Cargo.toml

# Runtime stage
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates libsqlite3-0 ffmpeg \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/server-rs /usr/local/bin/server-rs

EXPOSE 3000

CMD ["server-rs"]
