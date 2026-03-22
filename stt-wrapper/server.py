"""STT Service: Deepgram Nova-3 WebSocket proxy.

Accepts raw PCM audio (16kHz, 16-bit, mono) from the Rust server,
streams it to Deepgram Nova-3 via WebSocket, and emits
clean interim/final events back.

Extracts prosody features from buffered PCM, classifies emotion via
prosody analysis, and maps to ElevenLabs voice_settings style params.

Protocol (server → client):
  {"type": "interim", "text": "Hello world"}
  {"type": "final",   "text": "Hello world.", "emotion": "excited", "prosody": {...}, "style_params": {...}}
  {"type": "error",   "message": "..."}
"""

import asyncio
import json
import os
import logging

import numpy as np
import websockets

logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)s %(message)s")
log = logging.getLogger("stt")

from urllib.parse import urlparse, parse_qs

STT_PORT = int(os.environ.get("STT_PORT", "8766"))
DEEPGRAM_API_KEY = os.environ.get("DEEPGRAM_API_KEY", "")
STT_LANGUAGE = os.environ.get("STT_LANGUAGE", "en")
DEFAULT_SAMPLE_RATE = 44100


def build_dg_url(language: str, sample_rate: int = DEFAULT_SAMPLE_RATE) -> str:
    """Build Deepgram WebSocket URL for a given language and sample rate."""
    return (
        f"wss://api.deepgram.com/v1/listen"
        f"?model=nova-3"
        f"&encoding=linear16"
        f"&sample_rate={sample_rate}"
        f"&channels=1"
        f"&language={language}"
        f"&punctuate=true"
        f"&smart_format=true"
        f"&interim_results=true"
        f"&endpointing=400"
        f"&vad_events=true"
        f"&utterance_end_ms=1500"
    )


# Default URL for backwards compatibility
DG_WS_URL = build_dg_url(STT_LANGUAGE, DEFAULT_SAMPLE_RATE)


# ── Prosody Extraction ────────────────────────────────────

def extract_prosody(pcm_bytes: bytes, sample_rate: int = DEFAULT_SAMPLE_RATE) -> dict:
    """Extract prosody features from raw PCM (any sample rate, 16-bit, mono)."""
    if len(pcm_bytes) < 640:
        return {}

    n_samples = len(pcm_bytes) // 2
    samples = np.frombuffer(pcm_bytes, dtype=np.int16).astype(np.float32) / 32768.0
    duration_s = n_samples / sample_rate

    if duration_s < 0.1:
        return {}

    energy_rms = float(np.sqrt(np.mean(samples ** 2)))

    # Pitch estimation via autocorrelation
    frame_len = int(0.03 * sample_rate)
    hop_len = int(0.01 * sample_rate)
    pitches = []

    for start in range(0, len(samples) - frame_len, hop_len):
        frame = samples[start:start + frame_len]
        frame_energy = np.sum(frame ** 2)
        if frame_energy < 1e-6:
            continue

        corr = np.correlate(frame, frame, mode='full')
        corr = corr[len(corr) // 2:]

        min_lag = sample_rate // 500
        max_lag = sample_rate // 50

        if max_lag >= len(corr):
            continue

        segment = corr[min_lag:max_lag]
        if len(segment) == 0:
            continue

        peak_idx = np.argmax(segment) + min_lag
        if corr[0] > 0 and corr[peak_idx] / corr[0] > 0.3:
            pitch_hz = sample_rate / peak_idx
            if 50 < pitch_hz < 500:
                pitches.append(pitch_hz)

    pitch_mean = float(np.mean(pitches)) if pitches else 0.0
    pitch_std = float(np.std(pitches)) if pitches else 0.0

    # Pause density
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
        "speaking_rate_wpm": 0,
        "pause_density": round(pause_density, 3),
        "duration_s": round(duration_s, 2),
    }


def compute_speaking_rate(prosody: dict, word_count: int) -> dict:
    """Add speaking_rate_wpm based on transcript word count."""
    duration = prosody.get("duration_s", 0)
    if duration > 0 and word_count > 0:
        prosody["speaking_rate_wpm"] = round(word_count / duration * 60)
    return prosody


# ── Emotion Classification ──────────────────────────────

def classify_emotion(prosody: dict, sentiment: str, sentiment_score: float) -> str:
    """Classify emotion from prosody features (prosody-first, sentiment optional).

    Calibrated for web mic at arm's length (energy 0.01-0.05 RMS range).
    Speaking rate is unreliable (PCM buffer includes silence), so it's used
    only as a secondary signal.

    Returns one of: excited, happy, angry, sad, serious, neutral
    """
    energy = prosody.get("energy_rms", 0.025)
    pitch_std = prosody.get("pitch_std", 50)
    pitch_mean = prosody.get("pitch_mean", 150)
    pause_density = prosody.get("pause_density", 0.3)

    # Calibrated for actual web mic range
    # Observed: quiet=0.01, normal=0.02-0.03, loud=0.04+
    is_loud = energy > 0.035
    is_quiet = energy < 0.018
    is_normal_energy = not is_loud and not is_quiet

    # Pitch variation: observed range 50-120 std
    # Monotone < 55, normal 55-85, expressive > 85
    is_expressive = pitch_std > 85
    is_monotone = pitch_std < 55

    # High pitch suggests excitement/stress (relative)
    is_high_pitch = pitch_mean > 200

    # Pause density > 0.4 = lots of hesitation/sadness
    is_hesitant = pause_density > 0.4

    # Sentiment (may be neutral/0.0 if CF Workers AI doesn't pass it)
    has_sentiment = sentiment != "neutral" or abs(sentiment_score) > 0.1
    is_positive = sentiment == "positive" or sentiment_score > 0.3
    is_negative = sentiment == "negative" or sentiment_score < -0.3

    # Classification rules (prosody-first, sentiment boosts)
    if is_loud and is_expressive and (is_high_pitch or is_positive):
        return "excited"
    if is_loud and is_expressive:
        return "angry"  # loud + varied but not high pitch
    if is_loud and (is_positive or not is_monotone):
        return "happy"
    if is_quiet and is_monotone and is_hesitant:
        return "sad"
    if is_quiet and (is_hesitant or is_monotone):
        return "sad"
    if is_normal_energy and is_monotone:
        return "serious"
    if is_loud:
        return "happy"
    if is_expressive:
        return "happy"
    if has_sentiment and is_negative:
        return "sad"
    if has_sentiment and is_positive:
        return "happy"

    return "neutral"


# ── Style Param Mapping ──────────────────────────────────

# Aggressive emotion → ElevenLabs voice_settings mapping
EMOTION_STYLES = {
    "excited": {"stability": 0.20, "similarity_boost": 0.50, "style": 0.90, "speed": 1.20},
    "happy":   {"stability": 0.30, "similarity_boost": 0.60, "style": 0.70, "speed": 1.10},
    "angry":   {"stability": 0.25, "similarity_boost": 0.70, "style": 0.85, "speed": 1.05},
    "sad":     {"stability": 0.70, "similarity_boost": 0.80, "style": 0.40, "speed": 0.85},
    "serious": {"stability": 0.60, "similarity_boost": 0.80, "style": 0.30, "speed": 0.95},
    "neutral": {"stability": 0.50, "similarity_boost": 0.75, "style": 0.00, "speed": 1.00},
}


def map_style_params(emotion: str) -> dict:
    """Map classified emotion to ElevenLabs voice_settings."""
    params = EMOTION_STYLES.get(emotion, EMOTION_STYLES["neutral"]).copy()
    params["use_speaker_boost"] = True
    return params


# ── Sentiment Extraction from Deepgram ───────────────────

def extract_sentiment(data: dict) -> tuple[str, float]:
    """Extract sentiment from Deepgram response.

    Deepgram may include sentiment at channel, alternative, or segment level.
    Returns (sentiment_label, sentiment_score) or ("neutral", 0.0) if not available.
    """
    # Try channel.alternatives[0].sentiment
    try:
        channel = data.get("channel", {})
        alts = channel.get("alternatives", [])
        if alts:
            alt = alts[0]
            # Check for sentiment in the alternative
            if "sentiment" in alt:
                sent = alt["sentiment"]
                if isinstance(sent, dict):
                    return sent.get("sentiment", "neutral"), sent.get("sentiment_score", 0.0)
                elif isinstance(sent, str):
                    return sent, 0.0

            # Check for sentiments.segments or sentiments.average
            sentiments = alt.get("sentiments", {})
            if sentiments:
                avg = sentiments.get("average", {})
                if avg:
                    return avg.get("sentiment", "neutral"), avg.get("sentiment_score", 0.0)
                segs = sentiments.get("segments", [])
                if segs:
                    # Use last segment sentiment (most recent)
                    last = segs[-1]
                    return last.get("sentiment", "neutral"), last.get("sentiment_score", 0.0)
    except (KeyError, IndexError, TypeError):
        pass

    # Try top-level sentiments
    try:
        sentiments = data.get("sentiments", {})
        avg = sentiments.get("average", {})
        if avg:
            return avg.get("sentiment", "neutral"), avg.get("sentiment_score", 0.0)
    except (KeyError, TypeError):
        pass

    return "neutral", 0.0


# ── WebSocket Handling ────────────────────────────────────

async def connect_to_deepgram(language: str = None, sample_rate: int = DEFAULT_SAMPLE_RATE):
    """Connect to Deepgram Nova-3 WebSocket directly."""
    url = build_dg_url(language, sample_rate) if language else DG_WS_URL
    headers = {"Authorization": f"Token {DEEPGRAM_API_KEY}"}
    for attempt in range(1, 11):
        try:
            ws = await websockets.connect(
                url,
                additional_headers=headers,
                max_size=10 * 1024 * 1024,
            )
            log.info("Connected to Deepgram Nova-3 lang=%s (attempt %d)", language or STT_LANGUAGE, attempt)
            return ws
        except Exception as e:
            log.warning("Deepgram connect attempt %d/10 failed: %s", attempt, e)
            await asyncio.sleep(2)
    raise ConnectionError("Failed to connect to Deepgram after 10 attempts")


async def handle_client(client_ws):
    """Handle one client: proxy audio to Deepgram, emit STT events with emotion."""
    # Extract language from query params: /asr?lang=ko
    path = client_ws.request.path if hasattr(client_ws, 'request') else "/asr"
    parsed = urlparse(path)
    params = parse_qs(parsed.query)
    language = params.get("lang", [None])[0]
    sample_rate = int(params.get("sample_rate", [str(DEFAULT_SAMPLE_RATE)])[0])
    log.info("Client connected (lang=%s, sample_rate=%d)", language or STT_LANGUAGE, sample_rate)

    if not DEEPGRAM_API_KEY:
        log.error("DEEPGRAM_API_KEY must be set")
        await client_ws.send(json.dumps({
            "type": "error",
            "message": "STT not configured: missing DEEPGRAM_API_KEY"
        }))
        await client_ws.close()
        return

    try:
        dg_ws = await connect_to_deepgram(language, sample_rate)
    except ConnectionError as e:
        log.error("%s", e)
        await client_ws.send(json.dumps({"type": "error", "message": str(e)}))
        await client_ws.close()
        return

    last_interim = ""
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
        """Parse Deepgram responses and emit clean events with emotion."""
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
                        prosody = extract_prosody(bytes(pcm_buffer), sample_rate)
                        word_count = len(transcript.split())
                        prosody = compute_speaking_rate(prosody, word_count)

                        # Extract sentiment from Deepgram response
                        sentiment_label, sentiment_score = extract_sentiment(data)

                        # Classify emotion from prosody + sentiment
                        emotion = classify_emotion(prosody, sentiment_label, sentiment_score)

                        # Map emotion to ElevenLabs style params
                        style_params = map_style_params(emotion)

                        log.info("[EMOTION] %s (sentiment=%s/%.2f energy=%.4f rate=%dwpm pitch_std=%.1f)",
                                 emotion, sentiment_label, sentiment_score,
                                 prosody.get("energy_rms", 0),
                                 prosody.get("speaking_rate_wpm", 0),
                                 prosody.get("pitch_std", 0))
                        log.info("[STYLE] stability=%.2f similarity=%.2f style=%.2f speed=%.2f",
                                 style_params["stability"],
                                 style_params["similarity_boost"],
                                 style_params["style"],
                                 style_params["speed"])

                        await client_ws.send(json.dumps({
                            "type": "final",
                            "text": transcript,
                            "emotion": emotion,
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
    log.info("Using Deepgram Nova-3 direct (lang=%s)", STT_LANGUAGE)
    async with websockets.serve(handle_client, "0.0.0.0", STT_PORT):
        await asyncio.Future()


if __name__ == "__main__":
    asyncio.run(main())
