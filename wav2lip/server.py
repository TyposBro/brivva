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


class LipsyncRequest(BaseModel):
    audio_base64: str
    face_image_base64: str


class LipsyncResponse(BaseModel):
    frames_base64: list[str]
    fps: int
    lipsync_ms: int


def load_model():
    global model
    from models import Wav2Lip as Wav2LipModel
    import audio

    print(f"Loading Wav2Lip+GAN on {device}...")
    start = time.time()

    checkpoint_path = "/app/checkpoints/wav2lip_gan.pth"
    checkpoint = torch.load(checkpoint_path, map_location=device)

    model = Wav2LipModel()
    s = checkpoint["state_dict"]
    new_s = {k.replace("module.", ""): v for k, v in s.items()}
    model.load_state_dict(new_s)
    model = model.to(device).eval()

    print(f"Model loaded in {time.time() - start:.1f}s")


def decode_base64_image(b64: str) -> np.ndarray:
    img_bytes = base64.b64decode(b64)
    arr = np.frombuffer(img_bytes, dtype=np.uint8)
    img = cv2.imdecode(arr, cv2.IMREAD_COLOR)
    if img is None:
        raise HTTPException(status_code=400, detail="Invalid image data")
    return img


def encode_frame_jpeg(frame: np.ndarray, quality: int = 85) -> str:
    _, buf = cv2.imencode(".jpg", frame, [cv2.IMWRITE_JPEG_QUALITY, quality])
    return base64.b64encode(buf.tobytes()).decode("ascii")


def get_mel_chunks(wav_path: str, fps: int = 25):
    """Extract mel spectrogram chunks from audio, one per video frame."""
    import audio as wav2lip_audio

    wav = wav2lip_audio.load_wav(wav_path, 16000)
    mel = wav2lip_audio.melspectrogram(wav)

    # Each mel chunk covers one video frame
    mel_step_size = 16  # from Wav2Lip hparams
    mel_chunks = []
    i = 0
    fps_mel_ratio = 80.0 / fps  # 80 mel frames per second at 16kHz
    num_frames = int(len(wav) / 16000 * fps)

    for frame_idx in range(num_frames):
        start_idx = int(frame_idx * fps_mel_ratio)
        end_idx = start_idx + mel_step_size
        if end_idx > mel.shape[1]:
            # Pad with zeros
            chunk = np.zeros((mel.shape[0], mel_step_size))
            remaining = mel[:, start_idx:]
            chunk[:, :remaining.shape[1]] = remaining
        else:
            chunk = mel[:, start_idx:end_idx]
        mel_chunks.append(chunk)

    return mel_chunks


def detect_face(face_img: np.ndarray):
    """Detect face bbox using Wav2Lip's face detection."""
    from face_detection import FaceAlignment, LandmarksType
    detector = FaceAlignment(LandmarksType._2D, flip_input=False, device=device)
    predictions = detector.get_detections_for_batch(np.array([face_img]))
    if predictions[0] is None:
        return None
    return predictions[0]  # (x1, y1, x2, y2)


@app.post("/lipsync")
async def lipsync(req: LipsyncRequest) -> LipsyncResponse:
    start = time.time()

    face_img = decode_base64_image(req.face_image_base64)

    # Detect face
    bbox = detect_face(face_img)
    if bbox is None:
        raise HTTPException(status_code=400, detail="No face detected")

    y1, y2, x1, x2 = int(bbox[1]), int(bbox[3]), int(bbox[0]), int(bbox[2])
    # Pad bbox slightly
    pad = 10
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
            ["ffmpeg", "-y", "-i", mp3_path, "-ar", "16000", "-ac", "1", "-f", "wav", wav_path],
            capture_output=True, check=True,
        )
        mel_chunks = get_mel_chunks(wav_path, fps=25)
    finally:
        for p in [mp3_path, wav_path]:
            if os.path.exists(p):
                os.unlink(p)

    # Generate lip-synced frames
    frames_b64 = []
    batch_size = 16
    img_batch, mel_batch = [], []

    for i, mel_chunk in enumerate(mel_chunks):
        img_batch.append(face_crop.copy())
        mel_batch.append(mel_chunk)

        if len(img_batch) >= batch_size or i == len(mel_chunks) - 1:
            # Prepare image batch: mask lower half
            img_arr = np.array(img_batch)
            img_masked = img_arr.copy()
            img_masked[:, 96 // 2:, :, :] = 0  # mask lower half
            img_masked = img_masked / 255.0

            # Stack masked + original as 6-channel input
            img_input = np.concatenate([img_masked, img_arr / 255.0], axis=3)
            img_input = torch.FloatTensor(
                img_input.transpose(0, 3, 1, 2)
            ).to(device)

            mel_arr = np.array(mel_batch)
            mel_input = torch.FloatTensor(
                mel_arr[:, np.newaxis, :, :]
            ).to(device)

            with torch.no_grad():
                pred = model(mel_input, img_input)

            pred = (pred.cpu().numpy().transpose(0, 2, 3, 1) * 255).astype(np.uint8)

            for p in pred:
                # Resize prediction back to face bbox size and composite
                pred_resized = cv2.resize(p, (x2 - x1, y2 - y1))
                result = face_img.copy()
                result[y1:y2, x1:x2] = pred_resized
                frames_b64.append(encode_frame_jpeg(result))

            img_batch, mel_batch = [], []

    elapsed_ms = int((time.time() - start) * 1000)
    print(f"[WAV2LIP] {len(frames_b64)} frames ({elapsed_ms}ms)")

    return LipsyncResponse(
        frames_base64=frames_b64,
        fps=25,
        lipsync_ms=elapsed_ms,
    )


@app.get("/health")
async def health():
    return {
        "status": "healthy",
        "model": "wav2lip_gan",
        "device": device,
    }


@app.on_event("startup")
async def startup():
    load_model()


if __name__ == "__main__":
    uvicorn.run(app, host="0.0.0.0", port=8100)
