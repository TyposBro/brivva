FROM --platform=$BUILDPLATFORM rust:1.88-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
        cmake \
        build-essential \
        libclang-dev \
        python3 \
        python3-pip \
    && rm -rf /var/lib/apt/lists/* \
    && pip3 install --break-system-packages ziglang \
    && cargo install cargo-chef cargo-zigbuild --locked \
    && rustup target add x86_64-unknown-linux-gnu

WORKDIR /app
