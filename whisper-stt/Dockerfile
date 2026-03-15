FROM nvidia/cuda:12.4.1-runtime-ubuntu22.04

RUN apt-get update && apt-get install -y --no-install-recommends \
    python3.10 python3-pip ffmpeg \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

RUN pip install --no-cache-dir \
    whisperlivekit==0.2.19 \
    faster-whisper==1.2.1

RUN pip install --no-cache-dir --default-timeout=600 \
    torch==2.5.1 \
    torchaudio==2.5.1 \
    --extra-index-url https://download.pytorch.org/whl/cu124

# Pre-download whisper model at build time
RUN python3 -c "from faster_whisper import WhisperModel; WhisperModel('base.en', device='cpu', compute_type='int8')"

EXPOSE 8765

CMD ["whisperlivekit-server", \
     "--model", "base.en", \
     "--lan", "en", \
     "--port", "8765", \
     "--host", "0.0.0.0", \
     "--backend", "faster-whisper", \
     "--pcm-input", \
     "--no-vac"]
