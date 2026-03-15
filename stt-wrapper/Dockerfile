FROM nvidia/cuda:12.4.1-runtime-ubuntu22.04

RUN apt-get update && apt-get install -y --no-install-recommends \
    python3.10 python3-pip python3.10-dev ffmpeg portaudio19-dev \
    build-essential \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Install CUDA-enabled torch first
RUN pip install --no-cache-dir --default-timeout=600 \
    torch==2.5.1 \
    torchaudio==2.5.1 \
    --extra-index-url https://download.pytorch.org/whl/cu124

# Install RealtimeSTT + websockets
RUN pip install --no-cache-dir \
    RealtimeSTT==0.3.104

# Pre-download faster-whisper model at build time
RUN python3 -c "from faster_whisper import WhisperModel; WhisperModel('small.en', device='cpu', compute_type='int8')"

EXPOSE 8766

COPY server.py .

CMD ["python3", "server.py"]
