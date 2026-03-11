#!/usr/bin/env bash
set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
KOKORO_DIR="$SCRIPT_DIR/kokoro"

# Check UniDic dictionary (required for Japanese TTS)
if [ ! -f "$KOKORO_DIR/.venv/lib/python3.10/site-packages/unidic/dicdir/mecabrc" ]; then
  echo "Downloading UniDic dictionary (526MB, one-time)..."
  "$KOKORO_DIR/.venv/bin/python" -m unidic download
fi

# Free port 8880 if already in use
lsof -ti :8880 | xargs kill -9 2>/dev/null || true

# Start cloudflared tunnel in background
echo "Starting cloudflared tunnel..."
cloudflared tunnel --config ~/.cloudflared/brivva-kokoro.yml run &
TUNNEL_PID=$!

# Kill tunnel on exit
trap "kill $TUNNEL_PID 2>/dev/null" EXIT

# Start Kokoro (foreground — Ctrl+C stops both)
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
