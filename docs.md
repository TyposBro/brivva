# Brivva Technical Documentation

## v2 Architecture (current — rooms + multi-language)

```
Host Browser (/host)
  → Korean/English speech → PCM linear16 @ 16kHz (ScriptProcessorNode)
  → WebSocket /api/room?role=host&sourceLang=en → Worker generates roomId
  → Worker proxies WS to RoomDO (pinned to host's DC, e.g. Tokyo/Seoul)
  → DO sends room:created{roomId} back to host
  → Binary PCM frames → RoomDO

RoomDO (Durable Object — single actor per room, pinned to host's DC)
  → Nova-3 (Deepgram direct API if DEEPGRAM_API_KEY set, else CF AI Gateway language=en)
  → On is_final/speech_final:
      Promise.all([
        M2M100 sourceLang→en  (if EN guests),
        M2M100 sourceLang→ja  (if JA guests),
        M2M100 sourceLang→zh  (if ZH guests),
      ])
  → Per language: Kokoro TTS → stream audio chunks
  → Broadcast to all guests in that language group
  → Also sends interim/final to hostWs (host sees own transcription)

Guest Browser (/room/:id)
  → WebSocket /api/room?role=guest&roomId=ABC123&lang=en
  → Worker proxies WS to same RoomDO instance
  → Receives: interim, final, translation, tts_start, [MP3 chunks], tts_end
  → Blob URL playback (FIFO queue, no overlap)
```

## v1 Architecture (single-user, still deployed at /api/realtime)

```
Browser mic
  → PCM linear16 @ 16kHz (ScriptProcessorNode)
  → WebSocket → Cloudflare Worker
  → Nova-3 (Deepgram, streaming STT via CF AI Gateway WS)
  → M2M100-1.2B (CF Workers AI, translation EN→JA)
  → Kokoro 82M (self-hosted on M1 Pro MPS, voice: jf_alpha)
  → WebSocket binary → Browser Blob URL playback → speaker
```

## Worker Statefulness — Durable Objects

Each room is a `RoomDO` instance, keyed by room ID (`ROOMS.idFromName(roomId)`). The DO is a single-actor process — all WebSocket connections for a room land on the same DO instance regardless of which CF data center handled the incoming HTTP request.

**DO placement:** CF pins a DO to the data center that first created it — whichever DC handled the host's `?role=host` request. Since Brivva hosts are Korean beauty sellers, DOs are created in Seoul or Tokyo.

**Latency by market:**

| Audience | DC routing | WS round-trip |
|----------|-----------|---------------|
| Korean host | Seoul/Tokyo DC | ~5ms |
| Japanese guests | Same or adjacent DC | ~30–50ms |
| Chinese guests | Shanghai/Tokyo hop | ~40–80ms |
| English guests (US/EU) | Trans-Pacific | ~100–200ms |

The trans-Pacific hop for EN guests is a one-time WebSocket setup cost. Once connected, audio chunks stream from Tokyo — perceived latency for EN guests is dominated by the translation+TTS pipeline (~1–2s), not the extra ~150ms of network.

This DC placement is optimal for Brivva's primary markets (JA/ZH) and acceptable for secondary markets (EN). Exactly the right trade-off for K-Beauty live commerce.

### Limits

| Thing | Limit | Notes |
|-------|-------|-------|
| Room lifetime | As long as host WS is open | Open WS keeps isolate alive |
| Isolate eviction | ~30s after last request/message | All rooms wiped — guests see "closed" |
| Multiple data centers | Each DC has its own isolate | Host + guest must land on same DC |
| Worker CPU per request | 30s (paid) / 50ms (free) | WS handlers are event-driven, not a single long request |
| Concurrent WS connections | ~1000 per isolate (practical) | Fine for demo |
| Room persistence across restarts | None | In-memory only |

**For the demo:** 3 tabs on one machine → same data center → same isolate. Works.

**For production:** Durable Objects. One DO per room = single-instance actor, survives idle gaps, globally consistent. ~2h refactor.

---

## WebSocket Protocol (v2, /api/room)

**Host messages (client → server):**
```
{ type: "host:create", sourceLang: "ko" }   → server responds room:created
[ArrayBuffer]                               → PCM audio frames (same as v1)
{ type: "host:end" }                        → close room
```

**Guest messages (client → server):**
```
{ type: "guest:join", roomId: "ABC123", lang: "en"|"ja"|"zh" }
```

**Server → Host:**
```
{ type: "room:created",    roomId }
{ type: "room:guest_count", counts: { en: 5, ja: 3, zh: 12 } }
{ type: "interim",         transcript }
{ type: "final",           transcript, utteranceId }
```

**Server → Guest (same pipeline messages as v1):**
```
{ type: "room:joined",    roomId, lang }
{ type: "interim",        transcript }
{ type: "final",          transcript, utteranceId }
{ type: "translation",    text, utteranceId, translateMs }
{ type: "tts_start",      utteranceId }
[ArrayBuffer...]                                    ← raw MP3 chunks
{ type: "tts_end",        utteranceId, ttsMs }
{ type: "room:closed" }                             ← host disconnected
{ type: "error",          message }
```

## Fan-out Optimization

Translate ONCE per language, broadcast to all N guests:

```
utterance finalized
  → check active lang groups (e.g. EN: 3 guests, JA: 1 guest, ZH: 0)
  → Promise.all([translate sourceLang→en, translate sourceLang→ja])   ← parallel
  → for each lang in parallel:
      Kokoro TTS → stream chunks → broadcast to all N guests in group
```

50 JA guests = 1 translation + 1 TTS call. Not 50.

## M2M100 Language Codes

Source language is dynamic (`sourceLang` from host URL param, default `ko`):
- `source_lang: "ko"` → `target_lang: "en"` (Korean → English)
- `source_lang: "ko"` → `target_lang: "ja"` (Korean → Japanese)
- `source_lang: "ko"` → `target_lang: "zh"` (Korean → Chinese)
- `source_lang: "en"` → `target_lang: "ja"` (English → Japanese, for testing)

---

## Measured Latency (Mar 11 2026, v1 baseline)

From `FINAL` event to `TTS_END` (full pipeline, browser perspective):

| Condition | Translation | TTS (Kokoro) | Total from FINAL |
|-----------|-------------|--------------|-----------------|
| Short text | 394–870ms | 584–787ms | 1066–1214ms |
| Typical sentence | 394–870ms | 1369–2297ms | 1857–3032ms |
| Long sentence | ~2826ms | ~3407ms | ~3803ms |

v2 adds parallel translation overhead (negligible — same M2M100 call, just parallel). TTS fan-out runs concurrently per language.

---

## STT: Nova-3 vs Whisper

### Why Nova-3

Nova-3 is a **streaming** STT model — returns interim transcripts token-by-token while the user is still speaking. Enables:
- Live transcript display (words appear in real-time)
- Pipeline starts translating the moment speech ends (no extra wait)

Whisper is **batch-only** — requires the complete audio clip before transcription begins. No interims possible.

### Accuracy & Cost (Artificial Analysis, Mar 2026)

| Model | WER | Speed Factor | Price |
|-------|-----|--------------|-------|
| Whisper Large v3 Turbo (Groq) | 4.8% | 375.9x | $0.67/1000min |
| Whisper Large v3 (Fireworks) | 4.8% | 301.7x | $1.00/1000min |
| Nova-3 (Deepgram) | 6.5% | 222.6x | $4.30/1000min |

**Use Nova-3 for real-time streaming.** Streaming is non-negotiable for live translation UX.

### CF AI Gateway Limitation

CF AI Gateway's Nova-3 binding (`@cf/deepgram/nova-3`) only supports `language=en`. Using `language=ko` returns error 2002. Using `language=multi` connects but produces no Results.

**Fix:** When `DEEPGRAM_API_KEY` is set as a worker secret, `RoomDO.connectNova()` connects directly to `wss://api.deepgram.com/v1/listen` with the host's `sourceLang`. Without the key, it falls back to CF AI Gateway with `language=en`.

---

## TTS: Kokoro vs CF Aura-2-fr

### Kokoro 82M (self-hosted M1 Pro, voice: jf_alpha / af_bella / zf_xiaobei)

- Quality: Arena ELO ~1050, natural voices
- Latency: ~430ms short text, no cold starts, no rate limits
- Price: free (self-hosted)
- Open source (Apache 2.0)
- Requires `misaki[ja]` + UniDic dictionary for Japanese tokenization

### Verdict

**Kokoro self-hosted wins.** Better quality, lower latency, no cold starts, no rate limits.

---

## Key Implementation Notes

- **`is_final` trigger**: Nova-3 fires `is_final=true` for each finalized chunk during continuous speech. `speech_final` only fires on silence. Both trigger translation.
- **`state.pending` fallback**: `speech_final` sometimes arrives with empty transcript. Use last non-empty interim as fallback.
- **Host receives transcripts**: `handleNovaMessage` sends `interim`/`final` to both `hostWs` and all guests. Host sees Korean transcription cards in real time.
- **TTS queue**: `isTtsPlayingRef` prevents audio overlap. Clips play FIFO. All chunks buffered until `tts_end`, then played via Blob URL.
- **TTS queue freeze fix**: `audio.play()` rejection (browser autoplay policy) previously left `isTtsPlayingRef.current = true` forever. Fixed: `.catch()` now calls `onDone()`.
- **AudioContext unlock**: Guest language picker button calls `new AudioContext(); ctx.resume().then(() => ctx.close())` on click to satisfy browser autoplay policy before first `audio.play()`.
- **TTS playback — Blob URL not MSE**: `MediaSource.addSourceBuffer("audio/mpeg")` throws on Safari. Fixed by buffering all chunks → `new Blob(chunks, {type: "audio/mpeg"})` → `new Audio(blobUrl)`. Works everywhere.
- **`clientWs.send(value)` not `value.buffer`**: Uint8Array subview — `.buffer` references the underlying SharedArrayBuffer which may contain garbage outside the view's range.
- **ScriptProcessorNode**: Deprecated but universally supported. AudioWorklet is the modern alternative.
- **PCM encoding**: Web Audio captures Float32 [-1,1]. Nova-3 requires Int16. Convert: `Math.max(-32768, Math.min(32767, float32 * 32768))`.
- **Japanese TTS — UniDic required**: `python -m unidic download` (526MB). Missing data causes `MeCab initialization failed`.

## CF AI Gateway

Nova-3 connects via CF AI Gateway WebSocket (English-only fallback). Required headers:
- `Upgrade: websocket`
- `cf-aig-authorization: Bearer {CF_API_TOKEN}`

The token needs "Workers AI Run" + "AI Gateway Run" permissions on the CF account.

Direct Deepgram API (all languages):
- `Authorization: Token {DEEPGRAM_API_KEY}`

## Secrets

```
CF_ACCOUNT_ID    = 80a55132ae169d5b282ccf505bc66bf7
CF_AI_GATEWAY_ID = default
CF_API_TOKEN     = (wrangler secret) — CF AI Gateway auth
KOKORO_URL       = https://kokoro.milliytechnology.org
DEEPGRAM_API_KEY = (wrangler secret, optional) — enables Korean + multi-language STT
```

Update: `cd worker && echo "value" | npx wrangler secret put <NAME> --env=""`

## Kokoro Self-Hosted Setup

Runs on M1 Pro via Kokoro-FastAPI (`brivva/kokoro/`). Uses MPS (Apple Silicon GPU).

**One-time setup (Japanese TTS):**
```bash
cd ~/Documents/private/brivva/kokoro
.venv/bin/python -m unidic download   # 526MB — only needed once
```

**Start Kokoro + tunnel (one command):**
```bash
cd ~/Documents/private/brivva
bash kokoro-start.sh
```

Or manually:
```bash
cd ~/Documents/private/brivva/kokoro
USE_GPU=true USE_ONNX=false PYTHONPATH=$(pwd):$(pwd)/api MODEL_DIR=src/models \
VOICES_DIR=src/voices/v1_0 WEB_PLAYER_PATH=$(pwd)/web DEVICE_TYPE=mps \
PYTORCH_ENABLE_MPS_FALLBACK=1 uv run --no-sync uvicorn api.src.main:app --host 0.0.0.0 --port 8880
```

```bash
cloudflared tunnel --config ~/.cloudflared/brivva-kokoro.yml run
```

Tunnel config: `~/.cloudflared/brivva-kokoro.yml`
DNS: `kokoro.milliytechnology.org` → CNAME → `b6e5239e-b304-4087-8879-97fb649e6ba1.cfargotunnel.com` (proxied)

---

## v3 Architecture — Self-Hosted Rust Server (Mar 15 2026)

### Goal

Replace CF Workers with a single Rust (axum) server. Co-locate all ML services on one machine (Mac locally, EC2/ECS for production) to eliminate network round-trips between pipeline stages.

### Architecture

```
Host Browser (/host)
  → PCM linear16 @ 16kHz → WebSocket wss://brivva-server.milliytechnology.org/api/room?role=host&sourceLang=ko
  → server-rs (Rust/axum, port 3000)

server-rs (Rust, axum, DashMap rooms, mpsc channels)
  → WhisperLiveKit (STT, port 8765, localhost)
      - faster-whisper (CTranslate2) with streaming WebSocket
      - Deepgram-compatible API — swappable to real Deepgram later
      - Supports Korean, English, Japanese, Chinese, all languages
      - Model: whisper-large-v3-turbo (fast) or whisper-large-v3 (accurate)
  → NLLB-200 FastAPI (Translation, port 8000, localhost)
      - Meta's successor to M2M100: 3x faster, better quality, 200 languages
      - Model: facebook/nllb-200-distilled-600M (fast) or nllb-200-1.3B (accurate)
      - Single endpoint: POST /translate { text, source_lang, target_lang } → { translated_text }
      - Swappable: same API, swap model behind it
  → Kokoro FastAPI (TTS, port 8880, localhost)
      - Already working, no changes needed

Guest Browser (/room/:id)
  → WebSocket wss://brivva-server.milliytechnology.org/api/room?role=guest&roomId=ABC123&lang=ja
  → Receives: interim, final, translation, tts_start, [MP3 chunks], tts_end
```

### Service Ports (all localhost)

| Service | Port | Model | Purpose |
|---------|------|-------|---------|
| server-rs | 3000 | — | WebSocket routing, room state, pipeline orchestration |
| WhisperLiveKit | 8765 | whisper-large-v3-turbo | Streaming STT (WebSocket) |
| NLLB FastAPI | 8000 | nllb-200-distilled-600M | Translation (REST) |
| Kokoro FastAPI | 8880 | kokoro-v1_0 82M | TTS (REST, streaming response) |

### Why Self-Hosted Over API Services

| Component | Before (v2) | After (v3) | Latency saved |
|-----------|------------|------------|---------------|
| STT | Deepgram API (Seoul→US→Seoul) | WhisperLiveKit localhost | ~100-200ms network |
| Translation | CF Workers AI (edge, but still HTTP) | NLLB localhost | ~50-100ms network |
| TTS | Kokoro via cloudflared tunnel | Kokoro localhost | ~20-50ms tunnel overhead |
| **Total saved** | | | **~170-350ms per utterance** |

### Pipeline Flow (per utterance)

```
1. Host audio (PCM) → server-rs via WebSocket
2. server-rs → WhisperLiveKit WebSocket (localhost:8765)
   - Interim results → broadcast to host + all guests
   - Final result → trigger translation pipeline
3. For each active language (parallel tokio::join!):
   a. server-rs → NLLB FastAPI POST localhost:8000/translate
   b. Broadcast translation text to language group
   c. server-rs → Kokoro FastAPI POST localhost:8880/v1/audio/speech
   d. Stream MP3 chunks to language group
4. Send tts_end to language group
```

### Model Selection Rationale

**STT: WhisperLiveKit (faster-whisper)**
- Exposes Deepgram-compatible WebSocket API → drop-in replacement
- faster-whisper uses CTranslate2 (optimized inference, 4x faster than vanilla Whisper)
- Streaming with interim results (like Nova-3, unlike batch Whisper)
- whisper-large-v3-turbo: 5.4x speed improvement, 4 decoder layers instead of 32
- Runs on Apple MPS or CUDA

**Translation: NLLB-200 (distilled-600M)**
- 3x faster than M2M100-1.2B on same hardware
- Better translation quality (newer model, more training data)
- 200 languages vs M2M100's 100
- distilled-600M is half the size of M2M100-1.2B — fits easily on any GPU
- CC-BY-NC 4.0 license — fine for demo/interview, discuss with Brivva for production

**TTS: Kokoro 82M (unchanged)**
- Already working, already fast, already co-located

### AWS Deployment Plan

**Phase 1 — Demo (now):** Everything on MacBook Pro M1 (localhost)
**Phase 2 — Paid test:** Single EC2 g5.xlarge (1x A10G 24GB VRAM, ~$1/hr)
**Phase 3 — Production:** ECS with separate GPU tasks per service

```
# Phase 2: Single EC2
EC2 g5.xlarge (A10G 24GB VRAM)
├── server-rs          :3000  (CPU, ~10MB RAM)
├── WhisperLiveKit     :8765  (GPU, ~2GB VRAM)
├── NLLB FastAPI       :8000  (GPU, ~2GB VRAM)
├── Kokoro FastAPI     :8880  (GPU, ~1GB VRAM)
└── Total VRAM: ~5GB / 24GB available
```

```
# Phase 3: ECS (production)
ALB → ECS Service: server-rs (CPU task, auto-scale)
        ├→ ECS Service: WhisperLiveKit (GPU task, g5.xlarge)
        ├→ ECS Service: NLLB (GPU task, g5.xlarge or CPU with quantization)
        └→ ECS Service: Kokoro (GPU task, g5.xlarge)
```

**Why not Bedrock:** Bedrock only hosts foundation models (Claude, Llama, etc). Can't deploy Whisper, NLLB, or Kokoro on it.

### Swappability

Each service is behind a standard API (WebSocket or REST). To swap:
- **STT**: Change WebSocket URL from `localhost:8765` to Deepgram API or any Deepgram-compatible endpoint
- **Translation**: Change POST URL from `localhost:8000` to any endpoint accepting `{ text, source_lang, target_lang }`
- **TTS**: Change POST URL from `localhost:8880` to any endpoint returning audio stream
