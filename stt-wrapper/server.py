"""STT Service: Cloudflare Workers AI (Deepgram Nova-3) WebSocket proxy.

Accepts raw PCM audio (16kHz, 16-bit, mono) from the Rust server,
streams it to CF Workers AI Deepgram Nova-3 via WebSocket, and emits
clean interim/final events back.

Also extracts prosody features from buffered PCM on each "final" event
and maps them to ElevenLabs voice_settings style params.

Protocol (server → client):
  {"type": "interim", "text": "Hello world"}
  {"type": "final",   "text": "Hello world.", "prosody": {...}, "style_params": {...}}
  {"type": "error",   "message": "..."}
"""

import asyncio
import json
import os
import logging
import struct
import math

import numpy as np
import websockets

logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)s %(message)s")
log = logging.getLogger("stt")

STT_PORT = int(os.environ.get("STT_PORT", "8766"))
CF_ACCOUNT_ID = os.environ.get("CF_ACCOUNT_ID", "")
CF_API_TOKEN = os.environ.get("CF_API_TOKEN", "")
STT_LANGUAGE = os.environ.get("STT_LANGUAGE", "en")
SAMPLE_RATE = 16000

# Build Deepgram Nova-3 WebSocket URL via CF Workers AI
DG_WS_URL = (
    f"wss://api.cloudflare.com/client/v4/accounts/{CF_ACCOUNT_ID}"
    f"/ai/run/@cf/deepgram/nova-3"
    f"?encoding=linear16"
    f"&sample_rate={SAMPLE_RATE}"
    f"&channels=1"
    f"&language={STT_LANGUAGE}"
    f"&punctuate=true"
    f"&smart_format=true"
    f"&interim_results=true"
    f"&endpointing=400"
    f"&vad_events=true"
    f"&utterance_end_ms=1500"
)


# ── Prosody Extraction ────────────────────────────────────

def extract_prosody(pcm_bytes: bytes) -> dict:
    """Extract prosody features from raw PCM (16kHz, 16-bit, mono).

    Uses numpy only (no librosa dependency) for minimal latency.
    Returns: pitch_mean, pitch_std, energy_rms, speaking_rate_wpm, pause_density
    """
    if len(pcm_bytes) < 640:  # less than 20ms
        return {}

    # Convert PCM bytes to float32 array
    n_samples = len(pcm_bytes) // 2
    samples = np.frombuffer(pcm_bytes, dtype=np.int16).astype(np.float32) / 32768.0
    duration_s = n_samples / SAMPLE_RATE

    if duration_s < 0.1:
        return {}

    # Energy RMS (0.0 - 1.0)
    energy_rms = float(np.sqrt(np.mean(samples ** 2)))

    # Pitch estimation via autocorrelation (simplified)
    # Use 30ms frames, 10ms hop
    frame_len = int(0.03 * SAMPLE_RATE)  # 480 samples
    hop_len = int(0.01 * SAMPLE_RATE)    # 160 samples
    pitches = []

    for start in range(0, len(samples) - frame_len, hop_len):
        frame = samples[start:start + frame_len]
        frame_energy = np.sum(frame ** 2)
        if frame_energy < 1e-6:  # silence
            continue

        # Autocorrelation
        corr = np.correlate(frame, frame, mode='full')
        corr = corr[len(corr) // 2:]

        # Find first peak after minimum pitch period (50Hz = 320 samples at 16kHz)
        min_lag = SAMPLE_RATE // 500  # 500Hz max pitch = 32 samples
        max_lag = SAMPLE_RATE // 50   # 50Hz min pitch = 320 samples

        if max_lag >= len(corr):
            continue

        segment = corr[min_lag:max_lag]
        if len(segment) == 0:
            continue

        peak_idx = np.argmax(segment) + min_lag
        if corr[0] > 0 and corr[peak_idx] / corr[0] > 0.3:
            pitch_hz = SAMPLE_RATE / peak_idx
            if 50 < pitch_hz < 500:
                pitches.append(pitch_hz)

    pitch_mean = float(np.mean(pitches)) if pitches else 0.0
    pitch_std = float(np.std(pitches)) if pitches else 0.0

    # Pause density: ratio of silent frames to total frames
    frame_energies = []
    for start in range(0, len(samples) - frame_len, hop_len):
        frame = samples[start:start + frame_len]
        frame_energies.append(np.sum(frame ** 2) / frame_len)

    if frame_energies:
        threshold = np.median(frame_energies) * 0.1
        silent_frames = sum(1 for e in frame_energies if e < threshold)
        pause_density = silent_frames / len(frame_energies)
    else:
        pause_density = 0.0

    return {
        "pitch_mean": round(pitch_mean, 1),
        "pitch_std": round(pitch_std, 1),
        "energy_rms": round(energy_rms, 4),
        "speaking_rate_wpm": 0,  # placeholder — needs word count from transcript
        "pause_density": round(pause_density, 3),
        "duration_s": round(duration_s, 2),
    }


def compute_speaking_rate(prosody: dict, word_count: int) -> dict:
    """Add speaking_rate_wpm based on transcript word count."""
    duration = prosody.get("duration_s", 0)
    if duration > 0 and word_count > 0:
        prosody["speaking_rate_wpm"] = round(word_count / duration * 60)
    return prosody


# ── Style Param Mapping ──────────────────────────────────

def map_style_params(prosody: dict) -> dict:
    """Rule-based mapping: prosody → ElevenLabs voice_settings.

    high energy+fast → style↑ similarity↓ speed↑ (excited)
    low energy+slow  → stability↑ style↓ speed↓ (calm)
    """
    energy = prosody.get("energy_rms", 0.05)
    rate = prosody.get("speaking_rate_wpm", 150)
    pitch_std = prosody.get("pitch_std", 20)
    pause_density = prosody.get("pause_density", 0.3)

    # Normalize energy to 0-1 range (typical speech RMS: 0.01 - 0.15)
    energy_norm = min(1.0, max(0.0, (energy - 0.01) / 0.14))

    # Normalize rate (80-220 wpm typical range)
    rate_norm = min(1.0, max(0.0, (rate - 80) / 140))

    # Normalize pitch variation (higher std = more expressive)
    pitch_var_norm = min(1.0, max(0.0, (pitch_std - 5) / 50))

    # Combined expressiveness score
    expressiveness = (energy_norm * 0.4 + rate_norm * 0.3 + pitch_var_norm * 0.3)

    # Map to ElevenLabs params
    stability = 0.8 - (expressiveness * 0.5)       # 0.3 (expressive) - 0.8 (calm)
    similarity_boost = 0.9 - (expressiveness * 0.3) # 0.6 (expressive) - 0.9 (calm)
    style = expressiveness                           # 0.0 (calm) - 1.0 (expressive)
    speed = 0.9 + (rate_norm * 0.3)                 # 0.9 (slow) - 1.2 (fast)

    return {
        "stability": round(max(0.0, min(1.0, stability)), 2),
        "similarity_boost": round(max(0.0, min(1.0, similarity_boost)), 2),
        "style": round(max(0.0, min(1.0, style)), 2),
        "speed": round(max(0.8, min(1.3, speed)), 2),
        "use_speaker_boost": True,
    }


# ── WebSocket Handling ────────────────────────────────────

async def connect_to_deepgram():
    """Connect to CF Workers AI Deepgram Nova-3 WebSocket."""
    headers = {"Authorization": f"Bearer {CF_API_TOKEN}"}
    for attempt in range(1, 11):
        try:
            ws = await websockets.connect(
                DG_WS_URL,
                additional_headers=headers,
                max_size=10 * 1024 * 1024,
            )
            log.info("Connected to Deepgram Nova-3 (attempt %d)", attempt)
            return ws
        except Exception as e:
            log.warning("Deepgram connect attempt %d/10 failed: %s", attempt, e)
            await asyncio.sleep(2)
    raise ConnectionError("Failed to connect to Deepgram after 10 attempts")


async def handle_client(client_ws):
    """Handle one client: proxy audio to Deepgram, emit STT events with prosody."""
    log.info("Client connected")

    if not CF_ACCOUNT_ID or not CF_API_TOKEN:
        log.error("CF_ACCOUNT_ID and CF_API_TOKEN must be set")
        await client_ws.send(json.dumps({
            "type": "error",
            "message": "STT not configured: missing CF credentials"
        }))
        await client_ws.close()
        return

    try:
        dg_ws = await connect_to_deepgram()
    except ConnectionError as e:
        log.error("%s", e)
        await client_ws.send(json.dumps({"type": "error", "message": str(e)}))
        await client_ws.close()
        return

    last_interim = ""
    # Buffer PCM between finals for prosody extraction
    pcm_buffer = bytearray()

    async def forward_audio():
        """Forward binary audio from client to Deepgram."""
        nonlocal pcm_buffer
        try:
            async for msg in client_ws:
                if isinstance(msg, bytes):
                    pcm_buffer.extend(msg)
                    await dg_ws.send(msg)
        except websockets.ConnectionClosed:
            pass
        finally:
            try:
                await dg_ws.send(json.dumps({"type": "CloseStream"}))
            except Exception:
                pass

    async def parse_responses():
        """Parse Deepgram responses and emit clean events with prosody."""
        nonlocal last_interim, pcm_buffer
        try:
            async for msg in dg_ws:
                if isinstance(msg, bytes):
                    continue
                try:
                    data = json.loads(msg)
                except json.JSONDecodeError:
                    continue

                msg_type = data.get("type", "")

                if msg_type == "Results":
                    channel = data.get("channel", {})
                    alts = channel.get("alternatives", [])
                    if not alts:
                        continue

                    transcript = alts[0].get("transcript", "").strip()
                    if not transcript:
                        continue

                    is_final = data.get("is_final", False)
                    speech_final = data.get("speech_final", False)

                    if speech_final or is_final:
                        last_interim = ""
                        log.info("[FINAL] %s", transcript)

                        # Extract prosody from buffered PCM
                        prosody = extract_prosody(bytes(pcm_buffer))
                        word_count = len(transcript.split())
                        prosody = compute_speaking_rate(prosody, word_count)
                        style_params = map_style_params(prosody)

                        log.info("[PROSODY] energy=%.4f rate=%dwpm style=%.2f stability=%.2f",
                                 prosody.get("energy_rms", 0),
                                 prosody.get("speaking_rate_wpm", 0),
                                 style_params.get("style", 0),
                                 style_params.get("stability", 0))

                        await client_ws.send(json.dumps({
                            "type": "final",
                            "text": transcript,
                            "prosody": prosody,
                            "style_params": style_params,
                        }))

                        # Reset buffer for next utterance
                        pcm_buffer = bytearray()
                    else:
                        if transcript != last_interim:
                            last_interim = transcript
                            log.info("[INTERIM] %s", transcript)
                            await client_ws.send(json.dumps({
                                "type": "interim",
                                "text": transcript,
                            }))

                elif msg_type == "SpeechStarted":
                    log.info("VAD: speech started")

                elif msg_type == "UtteranceEnd":
                    log.info("VAD: utterance end")

                elif msg_type == "Metadata":
                    log.info("Deepgram session started (request_id=%s)",
                             data.get("request_id", "?"))

                elif msg_type == "Error":
                    log.error("Deepgram error: %s", data.get("message", ""))

        except websockets.ConnectionClosed:
            pass

    audio_task = asyncio.create_task(forward_audio())
    parse_task = asyncio.create_task(parse_responses())

    try:
        done, pending = await asyncio.wait(
            [audio_task, parse_task], return_when=asyncio.FIRST_COMPLETED
        )
        for task in pending:
            task.cancel()
    finally:
        try:
            await dg_ws.close()
        except Exception:
            pass
        log.info("Client disconnected")


async def main():
    log.info("STT server listening on ws://0.0.0.0:%d/asr", STT_PORT)
    log.info("Using CF Workers AI Deepgram Nova-3 (lang=%s)", STT_LANGUAGE)
    async with websockets.serve(handle_client, "0.0.0.0", STT_PORT):
        await asyncio.Future()


if __name__ == "__main__":
    asyncio.run(main())
