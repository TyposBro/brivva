#!/bin/bash
# Download all MuseTalk model weights at Docker build time
set -e

MODELS_DIR="/app/models"
mkdir -p "$MODELS_DIR"

dl() {
  local url="$1" dest="$2"
  mkdir -p "$(dirname "$dest")"
  echo "  ↓ $(basename "$dest")"
  wget -q --show-progress --progress=bar:force -O "$dest" "$url"
}

# ── MuseTalk weights (via git-xet clone, only has musetalk/ and musetalkV15/) ──

curl -sSfL https://hf.co/git-xet/install.sh | bash
git lfs install
git xet install

echo "=== Cloning MuseTalk model repo ==="
git clone https://huggingface.co/TMElyralab/MuseTalk /tmp/musetalk-weights

cp -r /tmp/musetalk-weights/musetalkV15 "$MODELS_DIR/musetalkV15"
rm -rf /tmp/musetalk-weights

# ── Other weights (not in the HF repo, download directly) ──

echo "=== dwpose ==="
dl "https://huggingface.co/camenduru/MuseTalk/resolve/main/dwpose/dw-ll_ucoco_384.pth" \
   "$MODELS_DIR/dwpose/dw-ll_ucoco_384.pth"

echo "=== face-parse-bisent ==="
dl "https://huggingface.co/camenduru/MuseTalk/resolve/main/face-parse-bisent/79999_iter.pth" \
   "$MODELS_DIR/face-parse-bisent/79999_iter.pth"

echo "=== resnet18 ==="
dl "https://download.pytorch.org/models/resnet18-5c106cde.pth" \
   "$MODELS_DIR/face-parse-bisent/resnet18-5c106cde.pth"

echo "=== sd-vae-ft-mse ==="
dl "https://huggingface.co/stabilityai/sd-vae-ft-mse/resolve/main/config.json" \
   "$MODELS_DIR/sd-vae/config.json"
dl "https://huggingface.co/stabilityai/sd-vae-ft-mse/resolve/main/diffusion_pytorch_model.bin" \
   "$MODELS_DIR/sd-vae/diffusion_pytorch_model.bin"

echo "=== whisper-tiny ==="
dl "https://huggingface.co/openai/whisper-tiny/resolve/main/config.json" \
   "$MODELS_DIR/whisper/config.json"
dl "https://huggingface.co/openai/whisper-tiny/resolve/main/pytorch_model.bin" \
   "$MODELS_DIR/whisper/pytorch_model.bin"
dl "https://huggingface.co/openai/whisper-tiny/resolve/main/preprocessor_config.json" \
   "$MODELS_DIR/whisper/preprocessor_config.json"

echo "=== s3fd face detector (runtime download, pre-cache) ==="
mkdir -p /root/.cache/torch/hub/checkpoints
dl "https://www.adrianbulat.com/downloads/python-fan/s3fd-619a316812.pth" \
   "/root/.cache/torch/hub/checkpoints/s3fd-619a316812.pth"

echo "=== Verifying ==="
du -sh "$MODELS_DIR"/*
