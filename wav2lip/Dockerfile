FROM nvidia/cuda:11.8.0-runtime-ubuntu22.04

ENV DEBIAN_FRONTEND=noninteractive
ENV PYTHONUNBUFFERED=1

# Layer 1: System packages
RUN apt-get update && apt-get install -y --no-install-recommends \
    python3.10 python3.10-dev python3-pip \
    && rm -rf /var/lib/apt/lists/*

# Layer 2: Tools
RUN apt-get update && apt-get install -y --no-install-recommends \
    git wget ffmpeg libgl1-mesa-glx libglib2.0-0 libsm6 libxrender1 libxext6 \
    && rm -rf /var/lib/apt/lists/*

RUN ln -sf /usr/bin/python3.10 /usr/bin/python3 && \
    ln -sf /usr/bin/python3.10 /usr/bin/python

WORKDIR /app

# Layer 3: PyTorch (cu118, same as musetalk for cache sharing)
RUN pip install --no-cache-dir --default-timeout=600 \
    torch==2.0.1 torchvision==0.15.2 torchaudio==2.0.2 \
    --index-url https://download.pytorch.org/whl/cu118

# Layer 4: Python deps
COPY requirements.txt .
RUN pip install --no-cache-dir -r requirements.txt

# Layer 5: Wav2Lip source
RUN git clone https://github.com/Rudrabha/Wav2Lip.git /app/Wav2Lip

# Layer 6: Model weights
COPY download_weights.sh .
RUN chmod +x download_weights.sh && bash download_weights.sh

# Layer 7: Our server (changes most often)
COPY server.py .

WORKDIR /app/Wav2Lip

EXPOSE 8100

CMD ["python3", "/app/server.py"]
