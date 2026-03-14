FROM python:3.10-slim

WORKDIR /app

RUN pip install --no-cache-dir \
    fastapi==0.135.1 \
    uvicorn==0.34.0 \
    transformers==4.47.0 \
    torch==2.5.1 \
    sentencepiece \
    protobuf

COPY server.py .

# Pre-download model at build time
RUN python -c "from transformers import AutoModelForSeq2SeqLM, AutoTokenizer; \
    AutoTokenizer.from_pretrained('facebook/nllb-200-distilled-600M'); \
    AutoModelForSeq2SeqLM.from_pretrained('facebook/nllb-200-distilled-600M')"

EXPOSE 8000

CMD ["python", "server.py"]
