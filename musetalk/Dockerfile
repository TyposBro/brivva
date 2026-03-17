FROM nvidia/cuda:11.8.0-devel-ubuntu22.04

ENV DEBIAN_FRONTEND=noninteractive
ENV PYTHONUNBUFFERED=1

# Layer 1: Core system packages
RUN apt-get update && apt-get install -y --no-install-recommends \
    python3.10 python3.10-dev python3-pip \
    && rm -rf /var/lib/apt/lists/*

# Layer 2: Build/download tools
RUN apt-get update && apt-get install -y --no-install-recommends \
    git git-lfs wget curl unzip \
    && rm -rf /var/lib/apt/lists/*

# Layer 3: Runtime libs (opencv, ffmpeg)
RUN apt-get update && apt-get install -y --no-install-recommends \
    ffmpeg libgl1-mesa-glx libglib2.0-0 libsm6 libxrender1 libxext6 \
    && rm -rf /var/lib/apt/lists/*

RUN ln -sf /usr/bin/python3.10 /usr/bin/python3 && \
    ln -sf /usr/bin/python3.10 /usr/bin/python

WORKDIR /app

# Layer 4: PyTorch (big, rarely changes)
RUN pip install --no-cache-dir --default-timeout=600 \
    torch==2.0.1 torchvision==0.15.2 torchaudio==2.0.2 \
    --index-url https://download.pytorch.org/whl/cu118

# Layer 5: MMLab ecosystem
RUN pip install --no-cache-dir -U openmim
RUN mim install mmengine
RUN pip install --no-cache-dir mmcv==2.0.1 -f https://download.openmmlab.com/mmcv/dist/cu118/torch2.0/index.html
RUN mim install "mmdet==3.1.0"
RUN mim install "mmpose==1.1.0"

# Layer 6: MuseTalk source code (rarely changes)
RUN git clone https://github.com/TMElyralab/MuseTalk.git /app/MuseTalk

# Layer 7: Model weights (~6GB, rarely changes — keep above frequently changing layers)
COPY download_weights.sh .
RUN chmod +x download_weights.sh && bash download_weights.sh

# Layer 8: Python deps (changes more often — below weights)
COPY requirements.txt .
RUN pip install --no-cache-dir -r requirements.txt

# Layer 9: Our server (changes most often, last layer)
COPY server.py .

# MuseTalk expects weights at ./models/ relative to repo root
RUN ln -s /app/models /app/MuseTalk/models

WORKDIR /app/MuseTalk

EXPOSE 8100

CMD ["python3", "/app/server.py"]
