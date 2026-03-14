#!/usr/bin/env bash
# Start WhisperLiveKit STT server on port 8765
# Uses faster-whisper with large-v3-turbo model
# Exposes Deepgram-compatible WebSocket API

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$SCRIPT_DIR"

.venv/bin/whisperlivekit-server \
  --model large-v3-turbo \
  --language "" \
  --port 8765 \
  --host 0.0.0.0
