"""
Wav2Lip Lip-Sync Server
Same API as MuseTalk for unified switching:
  POST /lipsync   — generate lip-synced frames from audio + face frame
  GET  /health    — health check
Runs on localhost:8100
"""

import base64
import time
import sys
import os
import tempfile

import cv2
import numpy as np
import torch
from fastapi import FastAPI, HTTPException
from pydantic import BaseModel
import uvicorn

sys.path.insert(0, "/app/Wav2Lip")

app = FastAPI()

device = "cuda" if torch.cuda.is_available() else "cpu"
model = None
face_detector = None
gfpgan_enhancer = None


class LipsyncRequest(BaseModel):
    audio_base64: str
    face_image_base64: str


class LipsyncResponse(BaseModel):
    frames_base64: list[str]
    fps: int
    lipsync_ms: int


def load_model():
    global model, face_detector, gfpgan_enhancer
    from models import Wav2Lip as Wav2LipModel
    from face_detection import FaceAlignment, LandmarksType

    print(f"Loading Wav2Lip+GAN on {device}...")
    start = time.time()

    checkpoint_path = "/app/checkpoints/wav2lip_gan.pth"
    checkpoint = torch.load(checkpoint_path, map_location=device)

    model = Wav2LipModel()
    s = checkpoint["state_dict"]
    new_s = {k.replace("module.", ""): v for k, v in s.items()}
    model.load_state_dict(new_s)
    model = model.to(device).eval()

    # Cache face detector (avoid re-creating per request)
    face_detector = FaceAlignment(LandmarksType._2D, flip_input=False, device=device)

    # Load GFPGAN for face enhancement
    print("Loading GFPGAN face enhancer...")
    try:
        from gfpgan import GFPGANer
        gfpgan_enhancer = GFPGANer(
            model_path="/app/gfpgan/GFPGANv1.4.pth",
            upscale=1,
            arch="clean",
            channel_multiplier=2,
            bg_upsampler=None,
            device=device,
        )
        print("GFPGAN loaded")
    except Exception as e:
        print(f"GFPGAN failed to load: {e} — falling back to raw Wav2Lip output")
        gfpgan_enhancer = None

    print(f"Models loaded in {time.time() - start:.1f}s")


def decode_base64_image(b64: str) -> np.ndarray:
    img_bytes = base64.b64decode(b64)
    arr = np.frombuffer(img_bytes, dtype=np.uint8)
    img = cv2.imdecode(arr, cv2.IMREAD_COLOR)
    if img is None:
        raise HTTPException(status_code=400, detail="Invalid image data")
    return img


def encode_frame_jpeg(frame: np.ndarray, quality: int = 90) -> str:
    _, buf = cv2.imencode(".jpg", frame, [cv2.IMWRITE_JPEG_QUALITY, quality])
    return base64.b64encode(buf.tobytes()).decode("ascii")


def get_mel_chunks(wav_path: str, fps: int = 25):
    """Extract mel spectrogram chunks from audio, one per video frame."""
    import audio as wav2lip_audio

    wav = wav2lip_audio.load_wav(wav_path, 16000)
    mel = wav2lip_audio.melspectrogram(wav)

    mel_step_size = 16
    mel_chunks = []
    fps_mel_ratio = 80.0 / fps
    num_frames = int(len(wav) / 16000 * fps)

    for frame_idx in range(num_frames):
        start_idx = int(frame_idx * fps_mel_ratio)
        end_idx = start_idx + mel_step_size
        if end_idx > mel.shape[1]:
            chunk = np.zeros((mel.shape[0], mel_step_size))
            remaining = mel[:, start_idx:]
            chunk[:, : remaining.shape[1]] = remaining
        else:
            chunk = mel[:, start_idx:end_idx]
        mel_chunks.append(chunk)

    return mel_chunks


def detect_face(face_img: np.ndarray):
    """Detect face bbox using cached face detector."""
    predictions = face_detector.get_detections_for_batch(np.array([face_img]))
    if predictions[0] is None:
        return None
    return predictions[0]


def create_feather_mask(h: int, w: int, border: int = 8) -> np.ndarray:
    """Create a feathered mask for smooth blending at bbox edges."""
    mask = np.ones((h, w), dtype=np.float32)
    for i in range(border):
        alpha = (i + 1) / border
        mask[i, :] *= alpha
        mask[h - 1 - i, :] *= alpha
        mask[:, i] *= alpha
        mask[:, w - 1 - i] *= alpha
    return mask[:, :, np.newaxis]


TARGET_FPS = int(os.environ.get("LIPSYNC_FPS", "60"))
WAV2LIP_FPS = 25


def enhance_face(face_region: np.ndarray) -> np.ndarray:
    """Enhance face using GFPGAN for sharper, more natural output."""
    if gfpgan_enhancer is None:
        return face_region
    try:
        _, _, output = gfpgan_enhancer.enhance(
            face_region,
            has_aligned=False,
            only_center_face=True,
            paste_back=True,
        )
        return output
    except Exception:
        return face_region


def interpolate_frames(frames: list[np.ndarray], target_fps: int, source_fps: int = 25) -> list[np.ndarray]:
    """Interpolate frames from source_fps to target_fps using weighted blending."""
    if target_fps <= source_fps or len(frames) < 2:
        return frames

    ratio = target_fps / source_fps
    total_out = int(len(frames) * ratio)
    out = []

    for i in range(total_out):
        src_pos = i / ratio
        idx = int(src_pos)
        frac = src_pos - idx

        if idx >= len(frames) - 1:
            out.append(frames[-1])
        elif frac < 0.001:
            out.append(frames[idx])
        else:
            # Weighted blend between consecutive frames
            a = frames[idx].astype(np.float32)
            b = frames[idx + 1].astype(np.float32)
            blended = cv2.addWeighted(frames[idx], 1.0 - frac, frames[idx + 1], frac, 0)
            out.append(blended)

    return out


@app.post("/lipsync")
async def lipsync(req: LipsyncRequest) -> LipsyncResponse:
    start = time.time()

    face_img = decode_base64_image(req.face_image_base64)

    # Detect face
    bbox = detect_face(face_img)
    if bbox is None:
        raise HTTPException(status_code=400, detail="No face detected")

    y1, y2, x1, x2 = int(bbox[1]), int(bbox[3]), int(bbox[0]), int(bbox[2])

    # Larger padding for better blending context
    pad = 20
    y1 = max(0, y1 - pad)
    x1 = max(0, x1 - pad)
    y2 = min(face_img.shape[0], y2 + pad)
    x2 = min(face_img.shape[1], x2 + pad)

    face_crop = cv2.resize(face_img[y1:y2, x1:x2], (96, 96))

    # Decode audio MP3 → WAV
    audio_bytes = base64.b64decode(req.audio_base64)
    with tempfile.NamedTemporaryFile(suffix=".mp3", delete=False) as f:
        f.write(audio_bytes)
        mp3_path = f.name

    wav_path = mp3_path.replace(".mp3", ".wav")
    try:
        import subprocess

        subprocess.run(
            [
                "ffmpeg", "-y", "-i", mp3_path,
                "-ar", "16000", "-ac", "1", "-f", "wav", wav_path,
            ],
            capture_output=True,
            check=True,
        )
        mel_chunks = get_mel_chunks(wav_path, fps=WAV2LIP_FPS)
    finally:
        for p in [mp3_path, wav_path]:
            if os.path.exists(p):
                os.unlink(p)

    # Generate lip-synced frames at 25fps
    raw_frames = []
    batch_size = 16
    img_batch, mel_batch = [], []

    # Pre-compute feather mask for blending
    bbox_h, bbox_w = y2 - y1, x2 - x1
    feather_mask = create_feather_mask(bbox_h, bbox_w, border=12)

    for i, mel_chunk in enumerate(mel_chunks):
        img_batch.append(face_crop.copy())
        mel_batch.append(mel_chunk)

        if len(img_batch) >= batch_size or i == len(mel_chunks) - 1:
            # Prepare image batch: mask lower half
            img_arr = np.array(img_batch)
            img_masked = img_arr.copy()
            img_masked[:, 96 // 2 :, :, :] = 0
            img_masked = img_masked / 255.0

            # Stack masked + original as 6-channel input
            img_input = np.concatenate([img_masked, img_arr / 255.0], axis=3)
            img_input = torch.FloatTensor(img_input.transpose(0, 3, 1, 2)).to(device)

            mel_arr = np.array(mel_batch)
            mel_input = torch.FloatTensor(mel_arr[:, np.newaxis, :, :]).to(device)

            with torch.no_grad():
                pred = model(mel_input, img_input)

            pred = (pred.cpu().numpy().transpose(0, 2, 3, 1) * 255).astype(np.uint8)

            for p in pred:
                # Resize prediction to bbox size
                pred_resized = cv2.resize(p, (bbox_w, bbox_h))

                # Enhance face with GFPGAN
                pred_resized = enhance_face(pred_resized)

                # Feathered blend into original frame
                result = face_img.copy()
                original_region = result[y1:y2, x1:x2].astype(np.float32)
                pred_float = pred_resized.astype(np.float32)
                blended = (pred_float * feather_mask + original_region * (1 - feather_mask))
                result[y1:y2, x1:x2] = blended.astype(np.uint8)

                raw_frames.append(result)

            img_batch, mel_batch = [], []

    # Interpolate 25fps → target fps
    if TARGET_FPS > WAV2LIP_FPS:
        out_frames = interpolate_frames(raw_frames, TARGET_FPS, WAV2LIP_FPS)
    else:
        out_frames = raw_frames

    frames_b64 = [encode_frame_jpeg(f) for f in out_frames]

    elapsed_ms = int((time.time() - start) * 1000)
    print(f"[WAV2LIP] {len(raw_frames)}@{WAV2LIP_FPS}fps → {len(frames_b64)}@{TARGET_FPS}fps ({elapsed_ms}ms)")

    return LipsyncResponse(
        frames_base64=frames_b64,
        fps=TARGET_FPS,
        lipsync_ms=elapsed_ms,
    )


@app.get("/health")
async def health():
    return {
        "status": "healthy",
        "model": "wav2lip_gan",
        "device": device,
        "gfpgan": gfpgan_enhancer is not None,
    }


@app.on_event("startup")
async def startup():
    load_model()


if __name__ == "__main__":
    uvicorn.run(app, host="0.0.0.0", port=8100)
