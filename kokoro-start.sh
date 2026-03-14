#!/usr/bin/env bash
set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
KOKORO_DIR="$SCRIPT_DIR/kokoro"
SERVER_DIR="$SCRIPT_DIR/server-rs"

# Check UniDic dictionary (required for Japanese TTS)
if [ ! -f "$KOKORO_DIR/.venv/lib/python3.10/site-packages/unidic/dicdir/mecabrc" ]; then
  echo "Downloading UniDic dictionary (526MB, one-time)..."
  "$KOKORO_DIR/.venv/bin/python" -m unidic download
fi

# Free ports if already in use
lsof -ti :8880 | xargs kill -9 2>/dev/null || true
lsof -ti :3000 | xargs kill -9 2>/dev/null || true

PIDS=()
cleanup() {
  echo ""
  echo "Shutting down..."
  for pid in "${PIDS[@]}"; do
    kill "$pid" 2>/dev/null || true
  done
  wait 2>/dev/null
}
trap cleanup EXIT

# 1. Start cloudflared tunnel for Kokoro
echo "Starting cloudflared tunnel (Kokoro)..."
cloudflared tunnel --config ~/.cloudflared/brivva-kokoro.yml run &
PIDS+=($!)

# 2. Start cloudflared tunnel for Rust server
echo "Starting cloudflared tunnel (server-rs on :3000)..."
cloudflared tunnel --url http://localhost:3000 &
PIDS+=($!)

# 3. Build and start Rust server
echo "Building server-rs..."
cd "$SERVER_DIR"
cargo build --release 2>&1
echo "Starting server-rs on :3000..."
./target/release/server-rs &
PIDS+=($!)
cd "$SCRIPT_DIR"

# 4. Start Kokoro (foreground — Ctrl+C stops everything)
echo "Starting Kokoro-FastAPI on :8880..."
cd "$KOKORO_DIR"
USE_GPU=true USE_ONNX=false \
PYTHONPATH="$KOKORO_DIR:$KOKORO_DIR/api" \
MODEL_DIR=src/models \
VOICES_DIR=src/voices/v1_0 \
WEB_PLAYER_PATH="$KOKORO_DIR/web" \
DEVICE_TYPE=mps \
PYTORCH_ENABLE_MPS_FALLBACK=1 \
uv run --no-sync uvicorn api.src.main:app --host 0.0.0.0 --port 8880
