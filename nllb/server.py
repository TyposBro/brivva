"""
NLLB-200 Translation Server
Single endpoint: POST /translate { text, source_lang, target_lang } → { translated_text }
Runs on localhost:8000
"""

from fastapi import FastAPI
from pydantic import BaseModel
from transformers import AutoModelForSeq2SeqLM, AutoTokenizer
import torch
import time
import uvicorn

app = FastAPI()

# NLLB uses BCP-47-like codes, not ISO 639-1
# Map our simple codes to NLLB's format
LANG_MAP = {
    "en": "eng_Latn",
    "ja": "jpn_Jpan",
    "zh": "zho_Hans",
    "ko": "kor_Hang",
    "fr": "fra_Latn",
}

MODEL_NAME = "facebook/nllb-200-distilled-600M"

print(f"Loading {MODEL_NAME}...")
tokenizer = AutoTokenizer.from_pretrained(MODEL_NAME)

# Use MPS on Apple Silicon, CUDA on GPU, else CPU
if torch.backends.mps.is_available():
    device = "mps"
elif torch.cuda.is_available():
    device = "cuda"
else:
    device = "cpu"

model = AutoModelForSeq2SeqLM.from_pretrained(MODEL_NAME).to(device)
print(f"Model loaded on {device}")


class TranslateRequest(BaseModel):
    text: str
    source_lang: str
    target_lang: str


class TranslateResponse(BaseModel):
    translated_text: str
    translate_ms: int


@app.post("/translate")
async def translate(req: TranslateRequest) -> TranslateResponse:
    src = LANG_MAP.get(req.source_lang, req.source_lang)
    tgt = LANG_MAP.get(req.target_lang, req.target_lang)

    start = time.time()

    tokenizer.src_lang = src
    inputs = tokenizer(req.text, return_tensors="pt").to(device)

    with torch.no_grad():
        output = model.generate(
            **inputs,
            forced_bos_token_id=tokenizer.convert_tokens_to_ids(tgt),
            max_new_tokens=256,
        )

    translated = tokenizer.decode(output[0], skip_special_tokens=True)
    elapsed_ms = int((time.time() - start) * 1000)

    return TranslateResponse(translated_text=translated, translate_ms=elapsed_ms)


@app.get("/health")
async def health():
    return {"status": "healthy", "model": MODEL_NAME, "device": device}


if __name__ == "__main__":
    uvicorn.run(app, host="0.0.0.0", port=8000)
