FROM python:3.10-slim

WORKDIR /app

RUN apt-get update && apt-get install -y --no-install-recommends \
    ffmpeg \
    && rm -rf /var/lib/apt/lists/*

RUN pip install --no-cache-dir \
    whisperlivekit==0.2.19 \
    faster-whisper==1.2.1 \
    torch==2.5.1 \
    torchaudio==2.5.1

# Pre-download whisper model at build time
RUN python -c "from faster_whisper import WhisperModel; WhisperModel('base.en', device='cpu', compute_type='int8')"

EXPOSE 8765

CMD ["whisperlivekit-server", \
     "--model", "base.en", \
     "--lan", "en", \
     "--port", "8765", \
     "--host", "0.0.0.0", \
     "--backend", "faster-whisper", \
     "--pcm-input", \
     "--no-vac"]
