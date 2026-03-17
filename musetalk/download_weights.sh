#!/bin/bash
# Download all MuseTalk model weights at Docker build time
set -e

MODELS_DIR="/app/models"
mkdir -p "$MODELS_DIR"

echo "=== Downloading MuseTalk v1.5 weights ==="
huggingface-cli download TMElyralab/MuseTalk \
    --local-dir "$MODELS_DIR/musetalk" \
    --include "musetalkV15/*" "models/dwpose/*" "models/face-parse-bisent/*"

# Reorganize: HF downloads into flat structure
if [ -d "$MODELS_DIR/musetalk/models/dwpose" ]; then
    mv "$MODELS_DIR/musetalk/models/dwpose" "$MODELS_DIR/dwpose"
fi
if [ -d "$MODELS_DIR/musetalk/models/face-parse-bisent" ]; then
    mv "$MODELS_DIR/musetalk/models/face-parse-bisent" "$MODELS_DIR/face-parse-bisent"
fi
if [ -d "$MODELS_DIR/musetalk/musetalkV15" ]; then
    mv "$MODELS_DIR/musetalk/musetalkV15" "$MODELS_DIR/musetalkV15"
fi
rm -rf "$MODELS_DIR/musetalk/models" "$MODELS_DIR/musetalk/.huggingface"

echo "=== Downloading sd-vae-ft-mse ==="
huggingface-cli download stabilityai/sd-vae-ft-mse \
    --local-dir "$MODELS_DIR/sd-vae" \
    --include "config.json" "diffusion_pytorch_model.bin"

echo "=== Downloading whisper-tiny ==="
huggingface-cli download openai/whisper-tiny \
    --local-dir "$MODELS_DIR/whisper" \
    --include "config.json" "pytorch_model.bin" "preprocessor_config.json"

echo "=== Downloading resnet18 ==="
mkdir -p "$MODELS_DIR/face-parse-bisent"
python3 -c "
import torch
from torchvision.models import resnet18, ResNet18_Weights
model = resnet18(weights=ResNet18_Weights.DEFAULT)
torch.save(model.state_dict(), '$MODELS_DIR/face-parse-bisent/resnet18-5c106cde.pth')
print('resnet18 saved')
"

echo "=== All weights downloaded ==="
du -sh "$MODELS_DIR"/*
