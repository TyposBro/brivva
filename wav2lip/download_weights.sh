#!/bin/bash
set -e

MODELS_DIR="/app/checkpoints"
mkdir -p "$MODELS_DIR"

dl() {
  local url="$1" dest="$2"
  mkdir -p "$(dirname "$dest")"
  echo "  ↓ $(basename "$dest")"
  wget -q --show-progress --progress=bar:force -O "$dest" "$url"
}

echo "=== Wav2Lip + GAN checkpoint (~436MB) ==="
dl "https://huggingface.co/Nekochu/Wav2Lip/resolve/main/wav2lip_gan.pth" \
   "$MODELS_DIR/wav2lip_gan.pth"

echo "=== s3fd face detection (~85MB) ==="
mkdir -p /app/Wav2Lip/face_detection/detection/sfd
dl "https://www.adrianbulat.com/downloads/python-fan/s3fd-619a316812.pth" \
   "/app/Wav2Lip/face_detection/detection/sfd/s3fd.pth"

echo "=== Done ==="
du -sh "$MODELS_DIR"/*
