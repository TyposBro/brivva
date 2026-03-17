"""
MuseTalk v1.5 Lip-Sync Server
Endpoints:
  POST /prepare   — preprocess a face image (avatar setup)
  POST /lipsync   — generate lip-synced frames from audio + prepared avatar
  GET  /health    — health check
Runs on localhost:8100
"""

import base64
import io
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
device = "cuda" if torch.cuda.is_available() else "cpu"
use_float16 = device == "cuda"
models = {}  # loaded at startup
avatars = {}  # avatar_id -> preprocessed data


# --- Pydantic models ---
class PrepareRequest(BaseModel):
    face_image_base64: str


class PrepareResponse(BaseModel):
    avatar_id: str
    prepare_ms: int


class LipsyncRequest(BaseModel):
    avatar_id: str
    audio_base64: str  # WAV or MP3 audio


class LipsyncResponse(BaseModel):
    frames_base64: list[str]  # JPEG frames
    fps: int
    lipsync_ms: int


# --- Model loading ---
def load_models():
    """Load all MuseTalk models at startup."""
    from musetalk.utils.utils import load_all_model
    from musetalk.utils.preprocessing import get_landmark_and_bbox
    from musetalk.utils.blending import get_image_prepare_material

    print(f"Loading MuseTalk v1.5 on {device} (float16={use_float16})...")
    start = time.time()

    model_dir = "/app/models"
    vae_dir = os.path.join(model_dir, "sd-vae")
    unet_config = os.path.join(model_dir, "musetalkV15", "musetalk.json")
    unet_path = os.path.join(model_dir, "musetalkV15", "unet.pth")

    audio_processor, vae, unet, pe = load_all_model(
        unet_model_path=unet_path,
        unet_config=unet_config,
        vae_dir=vae_dir,
        device=device,
        use_float16=use_float16,
    )

    models["audio_processor"] = audio_processor
    models["vae"] = vae
    models["unet"] = unet
    models["pe"] = pe

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


# --- Endpoints ---
@app.post("/prepare")
async def prepare(req: PrepareRequest) -> PrepareResponse:
    """Preprocess a face image for lip-sync (run once per avatar)."""
    from musetalk.utils.preprocessing import get_landmark_and_bbox
    from musetalk.utils.blending import get_image_prepare_material

    start = time.time()
    avatar_id = str(uuid.uuid4())[:8]

    face_img = decode_base64_image(req.face_image_base64)

    # Detect face landmarks and bbox
    landmarks, bboxes = get_landmark_and_bbox(
        [face_img], upperbondrange=0
    )
    if bboxes[0] is None:
        raise HTTPException(status_code=400, detail="No face detected in image")

    # Crop face region (256x256 for MuseTalk)
    x1, y1, x2, y2 = bboxes[0]
    crop = cv2.resize(face_img[y1:y2, x1:x2], (256, 256))

    # Prepare blending material
    prepare_material = get_image_prepare_material(face_img, bboxes[0])

    # Encode face latent via VAE
    vae = models["vae"]
    crop_tensor = (
        torch.from_numpy(crop)
        .permute(2, 0, 1)
        .unsqueeze(0)
        .float()
        .to(device)
        / 255.0
        * 2
        - 1
    )
    if use_float16:
        crop_tensor = crop_tensor.half()

    with torch.no_grad():
        latent = vae.encode(crop_tensor).latent_dist.sample()

    avatars[avatar_id] = {
        "face_img": face_img,
        "crop": crop,
        "bbox": bboxes[0],
        "landmark": landmarks[0],
        "latent": latent,
        "prepare_material": prepare_material,
    }

    elapsed_ms = int((time.time() - start) * 1000)
    return PrepareResponse(avatar_id=avatar_id, prepare_ms=elapsed_ms)


@app.post("/lipsync")
async def lipsync(req: LipsyncRequest) -> LipsyncResponse:
    """Generate lip-synced frames from audio + prepared avatar."""
    if req.avatar_id not in avatars:
        raise HTTPException(status_code=404, detail=f"Avatar {req.avatar_id} not found. Call /prepare first.")

    start = time.time()
    avatar = avatars[req.avatar_id]

    # Decode audio and convert to WAV 16kHz PCM (ElevenLabs sends MP3)
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
        frames = generate_lipsync_frames(avatar, wav_path)
    finally:
        for p in [mp3_path, wav_path]:
            if os.path.exists(p):
                os.unlink(p)

    # Encode frames as JPEG base64
    frames_b64 = [encode_frame_jpeg(f) for f in frames]

    elapsed_ms = int((time.time() - start) * 1000)
    return LipsyncResponse(
        frames_base64=frames_b64,
        fps=25,
        lipsync_ms=elapsed_ms,
    )


def generate_lipsync_frames(avatar: dict, audio_path: str) -> list[np.ndarray]:
    """Run MuseTalk inference: audio + avatar → lip-synced frames."""
    from musetalk.utils.utils import get_file_type, get_video_fps, datagen
    from musetalk.utils.blending import get_image_blending

    audio_processor = models["audio_processor"]
    vae = models["vae"]
    unet = models["unet"]
    pe = models["pe"]

    # Extract audio features using whisper
    whisper_feature = audio_processor.audio2feat(audio_path)
    whisper_chunks = audio_processor.feature2chunks(
        feature_array=whisper_feature, fps=25
    )

    face_latent = avatar["latent"]
    bbox = avatar["bbox"]
    face_img = avatar["face_img"]
    prepare_material = avatar["prepare_material"]

    output_frames = []

    for i, whisper_chunk in enumerate(whisper_chunks):
        # Prepare audio embedding
        audio_feat = torch.from_numpy(whisper_chunk).unsqueeze(0).to(device)
        if use_float16:
            audio_feat = audio_feat.half()

        # Create masked latent (mask lower half of face)
        masked_latent = face_latent.clone()
        masked_latent[:, :, masked_latent.shape[2] // 2 :, :] = 0

        # UNet forward pass
        with torch.no_grad():
            pred_latent = unet(
                masked_latent, timestep=torch.tensor([0], device=device),
                encoder_hidden_states=audio_feat,
            ).sample

        # Decode latent to image
        with torch.no_grad():
            pred = vae.decode(pred_latent).sample
            pred = (pred.clamp(-1, 1) + 1) / 2 * 255
            pred = pred[0].permute(1, 2, 0).cpu().numpy().astype(np.uint8)

        # Blend predicted face back onto original image
        result = get_image_blending(face_img, pred, prepare_material)
        output_frames.append(result)

    return output_frames


@app.get("/health")
async def health():
    return {
        "status": "healthy",
        "model": "musetalk_v1.5",
        "device": device,
        "float16": use_float16,
        "avatars_loaded": len(avatars),
    }


@app.on_event("startup")
async def startup():
    load_models()


if __name__ == "__main__":
    uvicorn.run(app, host="0.0.0.0", port=8100)
