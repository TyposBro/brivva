FROM python:3.12-slim-bookworm

RUN apt-get update && apt-get install -y --no-install-recommends \
    libsox-dev build-essential cmake libasound-dev \
    portaudio19-dev libportaudio2 libportaudiocpp0 ffmpeg \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /opt/fish-speech

COPY fish-speech/pyproject.toml fish-speech/README.md ./
COPY fish-speech/fish_speech ./fish_speech
COPY fish-speech/tools ./tools

RUN pip install --no-cache-dir -e .[stable]

EXPOSE 8080

ENTRYPOINT ["python", "tools/api_server.py"]
CMD ["--listen", "0.0.0.0:8080", \
     "--llama-checkpoint-path", "checkpoints/fish-speech-1.5", \
     "--decoder-checkpoint-path", "checkpoints/fish-speech-1.5/firefly-gan-vq-fsq-8x1024-21hz-generator.pth", \
     "--decoder-config-name", "firefly_gan_vq"]
