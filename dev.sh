#!/usr/bin/env bash
set -euo pipefail

# Brivva dev script — starts the full desktop app (Tauri + Axum + Vite)
# Usage: ./dev.sh          — normal dev mode
#        ./dev.sh build    — build .dmg for distribution
#        ./dev.sh check    — type-check frontend + backend without running

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$SCRIPT_DIR"

# Ensure .env.local exists
if [ ! -f .env.local ]; then
    echo "ERROR: .env.local not found. Create it with:"
    echo "  DEEPGRAM_API_KEY=..."
    echo "  GOOGLE_TRANSLATE_API_KEY=..."
    echo "  ELEVENLABS_API_KEY=..."
    exit 1
fi

# Ensure FFmpeg sidecar exists
SIDECAR="src-tauri/binaries/ffmpeg-aarch64-apple-darwin"
if [ ! -f "$SIDECAR" ]; then
    echo "Downloading FFmpeg sidecar..."
    mkdir -p src-tauri/binaries
    curl -L "https://evermeet.cx/ffmpeg/getrelease/ffmpeg/zip" -o /tmp/ffmpeg.zip
    unzip -o /tmp/ffmpeg.zip -d src-tauri/binaries/
    mv src-tauri/binaries/ffmpeg "$SIDECAR"
    chmod +x "$SIDECAR"
    rm /tmp/ffmpeg.zip
    echo "FFmpeg sidecar ready: $SIDECAR"
fi

# Ensure frontend deps installed
if [ ! -d frontend/node_modules ]; then
    echo "Installing frontend dependencies..."
    npm install --prefix frontend
fi

case "${1:-dev}" in
    dev)
        echo "Starting Brivva (dev mode)..."
        cd src-tauri
        ~/.cargo/bin/cargo-tauri dev
        ;;
    build)
        echo "Building Brivva .dmg..."
        cd src-tauri
        ~/.cargo/bin/cargo-tauri build
        echo "Done! Check src-tauri/target/release/bundle/"
        ;;
    check)
        echo "Type-checking..."
        cd frontend && npx tsc --noEmit && cd ..
        cargo check --workspace
        echo "All checks passed."
        ;;
    *)
        echo "Usage: ./dev.sh [dev|build|check]"
        exit 1
        ;;
esac
