# ── Multi-stage build for server-rs ──────────────────────────
# Build and run linux/amd64 so production ffmpeg behavior matches Ubuntu dev.
# Expensive build/runtime foundations are prebuilt:
#   - server-build-base: Rust + cargo-chef + cargo-zigbuild + ziglang
#   - server-runtime-base: Debian runtime libs + CJK fonts + custom ffmpeg
#
# Normal deploys should only run cargo-chef against dependency changes,
# compile server-rs, and copy the resulting binary into the runtime base.

ARG SERVER_BUILD_BASE_IMAGE=brivva-server-build-base-image-not-set-pass-build-arg:latest
ARG SERVER_RUNTIME_BASE_IMAGE=brivva-server-runtime-base-image-not-set-pass-build-arg:latest

FROM --platform=$BUILDPLATFORM ${SERVER_BUILD_BASE_IMAGE} AS chef
WORKDIR /app

# ── planner: extract dep recipe without building ─────────────
FROM chef AS planner
COPY Cargo.toml Cargo.lock ./
COPY server-rs/Cargo.toml server-rs/Cargo.toml
# Cargo needs target files to parse the package, but the dependency recipe
# should not change when application source changes. Keep dummy targets here
# so the expensive `cargo chef cook` layer keys only on manifests/lockfile.
RUN mkdir -p server-rs/src \
    && printf 'fn main() {}\n' > server-rs/src/main.rs \
    && printf '\n' > server-rs/src/lib.rs
RUN cargo chef prepare --recipe-path recipe.json

# ── builder: cook deps (cached), cross-compile app ───────────
FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --zigbuild \
    --target x86_64-unknown-linux-gnu --recipe-path recipe.json

COPY Cargo.toml Cargo.lock ./
COPY server-rs/Cargo.toml server-rs/Cargo.toml
COPY server-rs/src server-rs/src
RUN cargo zigbuild --release --target x86_64-unknown-linux-gnu \
    --manifest-path server-rs/Cargo.toml --bin server-rs

# ── runtime: amd64 slim debian with ffmpeg + subtitle fonts ──
FROM ${SERVER_RUNTIME_BASE_IMAGE}

COPY --from=builder /app/target/x86_64-unknown-linux-gnu/release/server-rs /usr/local/bin/server-rs

EXPOSE 3000

CMD ["server-rs"]
