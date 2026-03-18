"""
MuseTalk v1.5 Lip-Sync Server
Endpoints:
  POST /lipsync   — generate lip-synced frames from audio + face frame
  GET  /health    — health check
Runs on localhost:8100
"""

import base64
import math
import time
import sys
import os
import tempfile
import uuid

import cv2
import numpy as np
import torch
from fastapi import FastAPI, HTTPException
from pydantic import BaseModel
import uvicorn

# Add MuseTalk repo to path
sys.path.insert(0, "/app/MuseTalk")

app = FastAPI()

# --- Global state ---
device = torch.device("cuda" if torch.cuda.is_available() else "cpu")
use_float16 = device.type == "cuda"
weight_dtype = torch.float16 if use_float16 else torch.float32
models = {}  # loaded at startup


# --- Pydantic models ---
class LipsyncRequest(BaseModel):
    audio_base64: str  # MP3 audio from ElevenLabs
    face_image_base64: str  # Latest face frame from host webcam


class LipsyncResponse(BaseModel):
    frames_base64: list[str]  # JPEG frames
    fps: int
    lipsync_ms: int


# --- Model loading ---
def load_models():
    """Load all MuseTalk models at startup."""
    from musetalk.utils.utils import load_all_model
    from musetalk.utils.preprocessing import get_landmark_and_bbox
    from musetalk.utils.face_parsing import FaceParsing
    from musetalk.utils.audio_processor import AudioProcessor
    from transformers import WhisperModel

    print(f"Loading MuseTalk v1.5 on {device} (float16={use_float16})...")
    start = time.time()

    # Load VAE, UNet, PE
    vae, unet, pe = load_all_model(
        unet_model_path=os.path.join("models", "musetalkV15", "unet.pth"),
        vae_type="sd-vae",
        unet_config=os.path.join("models", "musetalkV15", "musetalk.json"),
        device=device,
    )

    # Convert to float16 if GPU
    if use_float16:
        pe = pe.half()
        vae.vae = vae.vae.half()
        unet.model = unet.model.half()

    pe = pe.to(device)
    vae.vae = vae.vae.to(device)
    unet.model = unet.model.to(device)

    # Audio processor + Whisper
    whisper_dir = os.path.join("models", "whisper")
    audio_processor = AudioProcessor(feature_extractor_path=whisper_dir)
    whisper = WhisperModel.from_pretrained(whisper_dir)
    whisper = whisper.to(device=device, dtype=weight_dtype).eval()
    whisper.requires_grad_(False)

    # Face parser for v1.5
    fp = FaceParsing()

    models["vae"] = vae
    models["unet"] = unet
    models["pe"] = pe
    models["audio_processor"] = audio_processor
    models["whisper"] = whisper
    models["fp"] = fp

    elapsed = time.time() - start
    print(f"Models loaded in {elapsed:.1f}s")


def decode_base64_image(b64: str) -> np.ndarray:
    """Decode base64 string to BGR numpy array."""
    img_bytes = base64.b64decode(b64)
    arr = np.frombuffer(img_bytes, dtype=np.uint8)
    img = cv2.imdecode(arr, cv2.IMREAD_COLOR)
    if img is None:
        raise HTTPException(status_code=400, detail="Invalid image data")
    return img


def encode_frame_jpeg(frame: np.ndarray, quality: int = 85) -> str:
    """Encode BGR numpy array to base64 JPEG."""
    _, buf = cv2.imencode(".jpg", frame, [cv2.IMWRITE_JPEG_QUALITY, quality])
    return base64.b64encode(buf.tobytes()).decode("ascii")


def detect_face_bbox(face_img: np.ndarray):
    """Detect face bbox + landmarks directly from numpy array (no file I/O)."""
    from musetalk.utils.preprocessing import model as mmpose_model, fa
    from mmpose.apis import inference_topdown
    from mmpose.structures import merge_data_samples

    # mmpose keypoint detection
    results = inference_topdown(mmpose_model, face_img)
    results = merge_data_samples(results)
    keypoints = results.pred_instances.keypoints
    face_land_mark = keypoints[0][23:91].astype(np.int32)

    # face detection for bbox
    bbox_det = fa.get_detections_for_batch(np.asarray([face_img]))
    if bbox_det[0] is None:
        return None

    # compute bbox from landmarks (same logic as get_landmark_and_bbox)
    half_face_coord = face_land_mark[29].copy()
    half_face_dist = np.max(face_land_mark[:, 1]) - half_face_coord[1]
    upper_bond = max(0, half_face_coord[1] - half_face_dist)

    x1 = int(np.min(face_land_mark[:, 0]))
    y1 = int(upper_bond)
    x2 = int(np.max(face_land_mark[:, 0]))
    y2 = int(np.max(face_land_mark[:, 1]))

    if y2 - y1 <= 0 or x2 - x1 <= 0 or x1 < 0:
        return bbox_det[0]  # fallback to raw detection bbox

    return (x1, y1, x2, y2)


def preprocess_face(face_img: np.ndarray):
    """Detect face, crop to 256x256, encode latent. Returns dict or None."""
    bbox = detect_face_bbox(face_img)
    if bbox is None:
        return None
    x1, y1, x2, y2 = bbox
    crop = cv2.resize(face_img[y1:y2, x1:x2], (256, 256))

    # VAE.get_latents_for_unet expects BGR numpy array — it preprocesses internally
    vae = models["vae"]
    with torch.no_grad():
        latent = vae.get_latents_for_unet(crop)

    return {"bbox": bbox, "latent": latent, "face_img": face_img}


# --- Endpoints ---
@app.post("/lipsync")
async def lipsync(req: LipsyncRequest) -> LipsyncResponse:
    """Generate lip-synced frames from audio + live face frame."""
    start = time.time()

    # Preprocess the face frame (detect, crop, encode latent)
    face_img = decode_base64_image(req.face_image_base64)
    face_data = preprocess_face(face_img)
    if face_data is None:
        raise HTTPException(status_code=400, detail="No face detected in image")

    prep_ms = int((time.time() - start) * 1000)

    # Decode audio: MP3 → WAV 16kHz PCM
    audio_bytes = base64.b64decode(req.audio_base64)
    with tempfile.NamedTemporaryFile(suffix=".mp3", delete=False) as f:
        f.write(audio_bytes)
        mp3_path = f.name

    wav_path = mp3_path.replace(".mp3", ".wav")
    try:
        import subprocess
        subprocess.run(
            ["ffmpeg", "-y", "-i", mp3_path, "-ar", "16000", "-ac", "1", "-f", "wav", wav_path],
            capture_output=True, check=True,
        )
        frames = generate_lipsync_frames(face_data, wav_path)
    finally:
        for p in [mp3_path, wav_path]:
            if os.path.exists(p):
                os.unlink(p)

    # Encode frames as JPEG base64
    frames_b64 = [encode_frame_jpeg(f) for f in frames]

    elapsed_ms = int((time.time() - start) * 1000)
    print(f"[LIPSYNC] {len(frames_b64)} frames (prep={prep_ms}ms, total={elapsed_ms}ms)")
    return LipsyncResponse(
        frames_base64=frames_b64,
        fps=25,
        lipsync_ms=elapsed_ms,
    )


@torch.no_grad()
def generate_lipsync_frames(face_data: dict, wav_path: str) -> list[np.ndarray]:
    """Run MuseTalk inference: audio + face → lip-synced frames."""
    from musetalk.utils.utils import datagen
    from musetalk.utils.blending import get_image

    vae = models["vae"]
    unet = models["unet"]
    pe = models["pe"]
    audio_processor = models["audio_processor"]
    whisper = models["whisper"]
    timesteps = torch.tensor([0], device=device)

    face_img = face_data["face_img"]
    bbox = face_data["bbox"]
    face_latent = face_data["latent"]

    # Extract audio features
    whisper_input_features, librosa_length = audio_processor.get_audio_feature(
        wav_path, weight_dtype=weight_dtype
    )
    if whisper_input_features is None:
        return []

    whisper_chunks = audio_processor.get_whisper_chunk(
        whisper_input_features, device, weight_dtype, whisper, librosa_length, fps=25
    )

    # Generate frames — each uses the same face latent (body position from current frame)
    output_frames = []
    num_frames = whisper_chunks.shape[0]
    fp = models["fp"]

    for i in range(num_frames):
        # Audio embedding for this frame
        audio_feat = whisper_chunks[i].unsqueeze(0)
        audio_feat = pe(audio_feat)

        # UNet forward pass — face_latent already has [masked, ref] from get_latents_for_unet
        pred_latent = unet.model(
            face_latent.to(dtype=unet.model.dtype), timesteps, encoder_hidden_states=audio_feat
        ).sample

        # Decode latent to image (returns uint8 BGR numpy)
        pred = vae.decode_latents(pred_latent)
        pred = pred[0]  # H x W x C numpy

        # Blend predicted face back using face parsing mask (seamless edges)
        x1, y1, x2, y2 = bbox
        pred_resized = cv2.resize(pred.astype(np.uint8), (x2 - x1, y2 - y1))
        result = get_image(face_img.copy(), pred_resized, [x1, y1, x2, y2], fp=fp)

        output_frames.append(result)

    return output_frames


@app.get("/health")
async def health():
    return {
        "status": "healthy",
        "model": "musetalk_v1.5",
        "device": str(device),
        "float16": use_float16,
    }


@app.on_event("startup")
async def startup():
    load_models()


if __name__ == "__main__":
    uvicorn.run(app, host="0.0.0.0", port=8100)
