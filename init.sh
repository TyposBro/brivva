#!/usr/bin/env bash
set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
KOKORO_DIR="$SCRIPT_DIR/kokoro"
SERVER_DIR="$SCRIPT_DIR/server-rs"
NLLB_DIR="$SCRIPT_DIR/nllb"
WHISPER_DIR="$SCRIPT_DIR/whisper-stt"
STT_WRAPPER_DIR="$SCRIPT_DIR/stt-wrapper"

# Check UniDic dictionary (required for Japanese TTS)
if [ ! -f "$KOKORO_DIR/.venv/lib/python3.10/site-packages/unidic/dicdir/mecabrc" ]; then
  echo "Downloading UniDic dictionary (526MB, one-time)..."
  "$KOKORO_DIR/.venv/bin/python" -m unidic download
fi

# Free ports if already in use
lsof -ti :8880 | xargs kill -9 2>/dev/null || true
lsof -ti :8766 | xargs kill -9 2>/dev/null || true
lsof -ti :8765 | xargs kill -9 2>/dev/null || true
lsof -ti :8000 | xargs kill -9 2>/dev/null || true
lsof -ti :3000 | xargs kill -9 2>/dev/null || true

PIDS=()
cleanup() {
  echo ""
  echo "Shutting down all services..."
  for pid in "${PIDS[@]}"; do
    kill "$pid" 2>/dev/null || true
  done
  wait 2>/dev/null
}
trap cleanup EXIT

# 1. Start cloudflared tunnel (Kokoro + server-rs)
echo "Starting cloudflared tunnel..."
cloudflared tunnel --config ~/.cloudflared/brivva-kokoro.yml run &
PIDS+=($!)

# 2. Start NLLB translation server (port 8000)
echo "Starting NLLB translation server on :8000..."
cd "$NLLB_DIR"
.venv/bin/python server.py &
PIDS+=($!)
cd "$SCRIPT_DIR"

# 3. Start WhisperLiveKit STT server (port 8765)
echo "Starting WhisperLiveKit STT on :8765..."
cd "$WHISPER_DIR"
.venv/bin/whisperlivekit-server \
  --model base.en \
  --lan en \
  --port 8765 \
  --host 0.0.0.0 \
  --backend mlx-whisper \
  --pcm-input \
  --no-vac &
PIDS+=($!)
cd "$SCRIPT_DIR"

# 4. Start STT wrapper (port 8766)
echo "Starting STT wrapper on :8766..."
cd "$STT_WRAPPER_DIR"
python server.py &
PIDS+=($!)
cd "$SCRIPT_DIR"

# 5. Build and start Rust server (port 3000)
echo "Building server-rs..."
cargo build --release --manifest-path "$SERVER_DIR/Cargo.toml" 2>&1
echo "Starting server-rs on :3000..."
"$SCRIPT_DIR/target/release/server-rs" &
PIDS+=($!)

# 6. Start Kokoro TTS (port 8880, foreground — Ctrl+C stops everything)
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
