# Brivva Technical Documentation

## Pipeline Overview

```
Browser mic
  → PCM linear16 @ 16kHz (ScriptProcessorNode)
  → WebSocket → Cloudflare Worker
  → Nova-3 (Deepgram, streaming STT via CF AI Gateway WS)
  → M2M100-1.2B (CF Workers AI, translation EN→FR)
  → Kokoro 82M (Replicate, TTS, voice: ff_siwis)
  → WebSocket binary → Browser AudioContext queue → speaker
```

## Measured Latency (Mar 11 2026)

From `FINAL` event to `TTS_END` (full pipeline, browser perspective):

| Condition | Translation | TTS (Kokoro) | Total from FINAL |
|-----------|-------------|--------------|-----------------|
| Short text | 394–870ms | 584–787ms | 1066–1214ms |
| Typical sentence | 394–870ms | 1369–2297ms | 1857–3032ms |
| Long sentence | ~2826ms | ~3407ms | ~3803ms |

**Translation (M2M100 on CF):** 394–870ms typical, near-zero network overhead. Up to 2826ms for longer sentences.

**TTS (Kokoro self-hosted on M1 Pro MPS):**
- Best case: 584ms (short text)
- Typical: 1369–2297ms
- No cold starts, no rate limits
- 20+ consecutive utterances stable in testing
- Occasional 502 from cloudflared tunnel (transient, self-resolving)

**vs Replicate:** M1 MPS is slower than Replicate A40 on average (typical 1400–2300ms vs 587ms warm) but eliminates cold starts (2–4s) and rate limiting (10–14s). Consistency wins for live demo use.

---

## STT: Nova-3 vs Whisper

### Why Nova-3

Nova-3 is a **streaming** STT model — it returns interim transcripts token-by-token while the user is still speaking. This enables:
- Live transcript display (words appear in real-time)
- Pipeline starts translating the moment speech ends (no extra wait)

Whisper is **batch-only** — it requires the complete audio clip before transcription begins. No interims possible.

### Accuracy & Cost (Artificial Analysis, Mar 2026)

| Model | WER | Speed Factor | Price |
|-------|-----|--------------|-------|
| Whisper Large v3 Turbo (Groq) | 4.8% | 375.9x | $0.67/1000min |
| Whisper Large v3 (Fireworks) | 4.8% | 301.7x | $1.00/1000min |
| Nova-3 (Deepgram) | 6.5% | 222.6x | $4.30/1000min |

Whisper is more accurate and cheaper in batch mode. Nova-3 is more expensive and slightly less accurate.

### Verdict

**Use Nova-3 for real-time streaming.** The streaming capability is non-negotiable for live translation UX — it's what makes interims visible and keeps pipeline latency tight. Whisper cannot replace it without fundamentally changing the product (VAD + batch → no live transcript).

Whisper is the right choice for batch transcription workloads (post-processing, analytics).

---

## TTS: Kokoro vs CF Aura-2-fr

### Kokoro 82M (Replicate, voice: ff_siwis)

- Quality: Arena ELO ~1050, natural French Swiss voice
- Warm latency: 587ms
- Cold start: 2–4s
- Rate limited: 10–14s
- Price: $0.65/1M chars
- Open source (Apache 2.0) — self-hostable

### CF Aura-2-fr (Deepgram, speaker: agathe)

- Not on Artificial Analysis TTS leaderboard (quality unknown)
- Expected latency: ~1.5–3s (based on CF aura-2-es measurements)
- Price: included in CF Workers AI usage
- No cold starts (serverless CF edge)
- Not self-hostable

### Verdict

Kokoro wins on quality and peak latency (587ms warm). CF Aura-2-fr wins on consistency (no cold starts or rate limits). CF Whisper background comparison was attempted but background tasks completed after WebSocket session closed — CF Workers AI GPU latency for batch Whisper is too slow to measure within a typical session.

**Next step:** Self-host Kokoro on M1 Pro → no cold starts, no rate limits, GPU-accelerated.

---

## Message Protocol (Worker ↔ Browser)

```
{ type: "interim",     transcript, utteranceId }          ← live words while speaking
{ type: "final",       transcript, utteranceId }          ← utterance finalized
{ type: "translation", text, utteranceId, translateMs }   ← French text + M2M100 ms
{ type: "tts_start",   utteranceId }                      ← audio stream starting
[ArrayBuffer...]                                          ← raw audio bytes (MP3)
{ type: "tts_end",     utteranceId, ttsMs }               ← audio done + Kokoro ms
{ type: "error",       message }                          ← pipeline error
```

## Key Implementation Notes

- **`is_final` trigger**: Nova-3 fires `is_final=true` for each finalized chunk during continuous speech. `speech_final` only fires on silence. Both trigger translation to keep pipeline responsive.
- **`state.pending` fallback**: `speech_final` sometimes arrives with empty transcript (endpointing signal only). Use last non-empty interim as fallback.
- **TTS queue**: `isTtsPlayingRef` prevents audio overlap. Clips play FIFO via `AudioContext.decodeAudioData`. Out-of-order delivery (concurrent Kokoro requests finishing at different times) can cause wrong playback order — self-hosted Kokoro with a serial queue fixes this.
- **`clientWs.send(value)` not `value.buffer`**: Uint8Array subview — `.buffer` references the underlying SharedArrayBuffer which may contain garbage outside the view's range.
- **`AudioContext.decodeAudioData`**: Format-agnostic MP3 decode. Works on Firefox/Brave/Chrome/Safari. `new Audio()` with Blob URL failed on Firefox.
- **ScriptProcessorNode**: Deprecated but universally supported. AudioWorklet is the modern alternative but requires more setup.
- **PCM encoding**: Web Audio captures Float32 [-1,1]. Nova-3 requires Int16 [-32768,32767] linear16. Convert per sample: `Math.max(-32768, Math.min(32767, float32 * 32768))`.

## CF AI Gateway

Nova-3 connects via CF AI Gateway WebSocket (not direct Deepgram API). Required headers:
- `Upgrade: websocket`
- `cf-aig-authorization: Bearer {CF_API_TOKEN}`

The token needs "Workers AI Run" + "AI Gateway Run" permissions on the CF account.

## Secrets

```
CF_ACCOUNT_ID    = 80a55132ae169d5b282ccf505bc66bf7
CF_AI_GATEWAY_ID = default
CF_API_TOKEN     = (wrangler secret) — CF AI Gateway auth
KOKORO_URL       = https://kokoro.milliytechnology.org
```

Update: `cd worker && npx wrangler secret put <NAME> --env=""`

## Kokoro Self-Hosted Setup

Runs on M1 Pro via Kokoro-FastAPI (`brivva/kokoro/`). Uses MPS (Apple Silicon GPU).

**Start Kokoro:**
```bash
cd ~/Documents/private/brivva/kokoro
USE_GPU=true USE_ONNX=false PYTHONPATH=$(pwd):$(pwd)/api MODEL_DIR=src/models \
VOICES_DIR=src/voices/v1_0 WEB_PLAYER_PATH=$(pwd)/web DEVICE_TYPE=mps \
PYTORCH_ENABLE_MPS_FALLBACK=1 uv run --no-sync uvicorn api.src.main:app --host 0.0.0.0 --port 8880
```

**Start tunnel:**
```bash
cloudflared tunnel --config ~/.cloudflared/brivva-kokoro.yml run
```

Tunnel config: `~/.cloudflared/brivva-kokoro.yml`
DNS: `kokoro.milliytechnology.org` → CNAME → `b6e5239e-b304-4087-8879-97fb649e6ba1.cfargotunnel.com` (proxied)
