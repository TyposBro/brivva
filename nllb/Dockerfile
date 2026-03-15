FROM nvidia/cuda:12.4.1-runtime-ubuntu22.04

RUN apt-get update && apt-get install -y --no-install-recommends \
    python3.10 python3-pip \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

RUN pip install --no-cache-dir \
    fastapi==0.135.1 \
    uvicorn==0.34.0 \
    transformers==4.47.0 \
    sentencepiece \
    protobuf

RUN pip install --no-cache-dir --default-timeout=600 \
    torch==2.5.1 --extra-index-url https://download.pytorch.org/whl/cu124

COPY server.py .

# Pre-download model at build time
RUN python3 -c "from transformers import AutoModelForSeq2SeqLM, AutoTokenizer; \
    AutoTokenizer.from_pretrained('facebook/nllb-200-distilled-600M'); \
    AutoModelForSeq2SeqLM.from_pretrained('facebook/nllb-200-distilled-600M')"

EXPOSE 8000

CMD ["python3", "server.py"]
