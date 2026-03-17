FROM nvidia/cuda:11.8.0-devel-ubuntu22.04

ENV DEBIAN_FRONTEND=noninteractive
ENV PYTHONUNBUFFERED=1

# System deps (including ffmpeg for audio processing)
RUN apt-get update && apt-get install -y --no-install-recommends \
    python3.10 python3.10-dev python3-pip \
    git wget ffmpeg libgl1-mesa-glx libglib2.0-0 libsm6 libxrender1 libxext6 \
    && rm -rf /var/lib/apt/lists/*

RUN ln -sf /usr/bin/python3.10 /usr/bin/python3 && \
    ln -sf /usr/bin/python3.10 /usr/bin/python

WORKDIR /app

# PyTorch 2.0.1 + CUDA 11.8 (matches MuseTalk's tested environment)
RUN pip install --no-cache-dir --default-timeout=600 \
    torch==2.0.1 torchvision==0.15.2 torchaudio==2.0.2 \
    --index-url https://download.pytorch.org/whl/cu118

# MMLab ecosystem — use prebuilt wheels (no source compilation)
RUN pip install --no-cache-dir -U openmim && \
    mim install mmengine && \
    pip install --no-cache-dir mmcv==2.0.1 -f https://download.openmmlab.com/mmcv/dist/cu118/torch2.0/index.html && \
    mim install "mmdet==3.1.0" && \
    mim install "mmpose==1.1.0"

# Python deps
COPY requirements.txt .
RUN pip install --no-cache-dir -r requirements.txt

# Clone MuseTalk repo (pinned commit for reproducibility)
RUN git clone https://github.com/TMElyralab/MuseTalk.git /app/MuseTalk

# Download model weights
COPY download_weights.sh .
RUN chmod +x download_weights.sh && bash download_weights.sh

# Copy our FastAPI server
COPY server.py .

EXPOSE 8100

CMD ["python3", "server.py"]
