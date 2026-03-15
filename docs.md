# Brivva Technical Documentation

## v3 Architecture — Self-Hosted Rust Server (current, Mar 15 2026)

Fully self-hosted pipeline. All ML services co-located on one machine (Mac locally, EC2/ECS for production). No cloud API dependencies.

### Architecture

```
Host Browser (/host)
  → PCM linear16 @ 16kHz → WebSocket wss://brivva-server.milliytechnology.org/api/room?role=host&sourceLang=en
  → server-rs (Rust/axum, port 3000)

server-rs (Rust, axum, DashMap rooms, mpsc channels)
  → STT Wrapper (stt-wrapper:8766, Python asyncio WebSocket proxy)
      - Proxies audio to CF Nova-3 (via CF AI Gateway)
      - Emits clean { type: "interim"/"final", text } JSON events
      - Handles silence stripping, sentence boundary detection, dedup
      - Env: CF_ACCOUNT_ID, CF_API_TOKEN, STT_LANGUAGE
  → NLLB-200 FastAPI (Translation, port 8000, localhost)
      - Model: facebook/nllb-200-distilled-600M
      - POST /translate { text, source_lang, target_lang } → { translated_text, translate_ms }
      - 82–164ms on GPU (warm)
  → ElevenLabs TTS (API, eleven_flash_v2_5)
      - POST /v1/text-to-speech/{voice_id}/stream → streaming MP3
      - 548–1,440ms depending on text length
      - Voices: Sarah (EN), Lily (JA), Alice (ZH), Jessica (KO)
      - 32 languages supported, no local GPU needed

Guest Browser (/room/:id)
  → WebSocket wss://brivva-server.milliytechnology.org/api/room?role=guest&roomId=ABC123&lang=ja
  → Receives: interim, final, translation, tts_start, [MP3 chunks], tts_end
  → Blob URL playback (FIFO queue, no overlap)
```

### Service Ports (all localhost)

| Service | Port | Model | Purpose |
|---------|------|-------|---------|
| server-rs | 3000 | — | WebSocket routing, room state, pipeline orchestration |
| stt-wrapper | 8766 | CF Nova-3 (via CF AI Gateway) | STT proxy (clean events) |
| NLLB FastAPI | 8000 | nllb-200-distilled-600M | Translation (REST, 82–164ms) |
| ElevenLabs | API | eleven_flash_v2_5 | TTS (streaming MP3, 548–1,440ms) |

### Pipeline Flow (per utterance)

```
1. Host audio (PCM Int16 s16le) → server-rs via WebSocket
2. server-rs → stt-wrapper WebSocket (localhost:8766)
   - stt-wrapper proxies to CF Nova-3 via CF AI Gateway
   - Returns clean { type: "interim"/"final", text } events
   - Interim results → broadcast to host + all guests
   - Final result → trigger translation pipeline
3. For each active language (parallel tokio::spawn):
   a. server-rs → NLLB POST localhost:8000/translate
   b. Broadcast translation text to language group + host
   c. server-rs → ElevenLabs POST api.elevenlabs.io/v1/text-to-speech/{voice_id}/stream
   d. Send tts_start to language group
   e. Stream MP3 chunks to language group as they arrive
   f. Send tts_end to language group + host
```

### Fan-out Optimization

Translate ONCE per language, broadcast to all N guests:

```
utterance finalized
  → check active lang groups (e.g. EN: 3 guests, JA: 1 guest, ZH: 0)
  → tokio::spawn per language (parallel)
  → for each lang:
      NLLB translate → ElevenLabs TTS → stream chunks → broadcast to all N guests in group
```

50 JA guests = 1 translation + 1 TTS call. Not 50.

### Why Self-Hosted Over API Services

| Component | Before (v2, CF Workers) | After (v3, localhost) | Latency saved |
|-----------|------------------------|----------------------|---------------|
| STT | Deepgram API (Seoul→US→Seoul) | WhisperLiveKit localhost | ~100-200ms network |
| Translation | CF Workers AI M2M100 | NLLB localhost | ~50-100ms network |
| TTS | Kokoro via cloudflared tunnel | ElevenLabs API | API-based (no local GPU) |
| **Total saved** | | | **~170-350ms per utterance** |

---

## Running

### Local (Mac M1 Pro — uses MPS acceleration)

```bash
cd ~/Documents/private/brivva
bash init.sh
```

Starts: cloudflared tunnel, NLLB (:8000), stt-wrapper (:8766), server-rs (:3000).

### Docker (Linux with NVIDIA GPU)

```bash
docker compose up --build
```

All services use `nvidia/cuda:12.4.1` base images with CUDA 12.4 + PyTorch cu124. Requires:
- NVIDIA GPU + drivers
- `nvidia-container-toolkit`
- NixOS: `hardware.nvidia-container-toolkit.enable = true; virtualisation.docker.enable = true;`

### Frontend

```bash
cd frontend && npm run deploy   # Cloudflare Pages
```

Live at: https://brivva.pages.dev

---

## Docker Services

| Service | Base Image | GPU | Notes |
|---------|-----------|-----|-------|
| server-rs | rust:1.85 → debian:bookworm-slim | No | Pipeline orchestration |
| stt-wrapper | python:3.10-slim | No | CF Nova-3 API proxy |
| nllb | nvidia/cuda:12.4.1-runtime | Yes (optional) | Auto-detects CPU/CUDA |
| ElevenLabs | — (external API) | No | No container needed |

### AWS Deployment Plan

**Phase 1 — Demo (now):** MacBook Pro M1 via `bash init.sh`
**Phase 2 — Paid test:** Single EC2 g5.xlarge (1x A10G 24GB VRAM, ~$1/hr)
**Phase 3 — Production:** ECS with separate GPU tasks per service

```
# Phase 2: Single EC2
EC2 g5.xlarge (A10G 24GB VRAM)
├── server-rs          :3000  (CPU, ~10MB RAM)
├── stt-wrapper        :8766  (CPU, CF Nova-3 API)
├── NLLB FastAPI       :8000  (GPU, ~2GB VRAM)
├── ElevenLabs         API    (external, no local resources)
└── Total VRAM: ~2GB / 24GB available
```

```
# Phase 3: ECS (production)
ALB → ECS Service: server-rs (CPU task, auto-scale)
        ├→ ECS Service: stt-wrapper (CPU task, CF Nova-3 API)
        ├→ ECS Service: NLLB (GPU task, g5.xlarge or CPU with quantization)
        └→ ElevenLabs TTS (external API, no ECS task needed)
```

---

## WebSocket Protocol (v3, /api/room)

Connection via query params (no JSON handshake):
- Host: `ws://host/api/room?role=host&sourceLang=en`
- Guest: `ws://host/api/room?role=guest&roomId=ABC123&lang=ja`

**Host messages (client → server):**
```
[ArrayBuffer]                               → PCM audio frames (Int16 s16le @ 16kHz)
{ "host:end" }                              → text message containing "host:end" closes room
```

**Server → Host:**
```
{ type: "room:created",     roomId }
{ type: "room:guest_count", counts: { en: 5, ja: 3, zh: 12 } }
{ type: "interim",          transcript }
{ type: "final",            transcript, utteranceId }
{ type: "translation",      text, utteranceId, translateMs }
{ type: "tts_end",          utteranceId, ttsMs }
```

**Server → Guest:**
```
{ type: "room:joined",     roomId }
{ type: "interim",         transcript }
{ type: "final",           transcript, utteranceId }
{ type: "translation",     text, utteranceId, translateMs }
{ type: "tts_start",       utteranceId }
[ArrayBuffer...]                                    ← raw MP3 chunks
{ type: "tts_end",         utteranceId, ttsMs }
{ type: "room:closed" }                             ← host disconnected
{ type: "error",           message }
```

---

## Model Selection Rationale

**STT: CF Nova-3 (via stt-wrapper)**
- stt-wrapper proxies audio to CF AI Gateway (Deepgram Nova-3)
- Returns clean interim/final JSON events with sentence boundary detection
- No local GPU needed for STT — runs via Cloudflare API
- Env vars: CF_ACCOUNT_ID, CF_API_TOKEN, STT_LANGUAGE

**Translation: NLLB-200 (distilled-600M)**
- Meta's successor to M2M100: better quality, 200 languages
- distilled-600M is half the size of M2M100-1.2B
- Auto-detects MPS/CUDA/CPU via PyTorch
- CC-BY-NC 4.0 license — fine for demo, discuss for production

**TTS: ElevenLabs (eleven_flash_v2_5)**
- 32-language support with natural voices
- API-based: no local GPU needed, no model downloads
- Streaming MP3 response
- Voice IDs: Sarah (EN), Lily (JA), Alice (ZH), Jessica (KO)
- Tradeoff: API dependency + per-character cost

### Swappability

Each service is behind a standard API (WebSocket or REST). To swap:
- **STT**: Change `STT_HOST`/`STT_PORT` env vars — any WebSocket that emits `{ type: "interim"/"final", text }` events
- **Translation**: Change `NLLB_HOST` env var — any `POST /translate { text, source_lang, target_lang }` endpoint
- **TTS**: Change `ELEVENLABS_API_KEY` env var — uses ElevenLabs API, or swap to any streaming TTS endpoint

---

## Key Implementation Notes

- **stt-wrapper**: Clean proxy between server-rs and CF Nova-3. Handles silence stripping, sentence boundaries, dedup. server-rs just receives clean interim/final events.
- **STT retry loop**: stt-wrapper may still be starting. server-rs retries connection 10 times, 3s apart.
- **TTS playback — Blob URL**: MSE `addSourceBuffer("audio/mpeg")` throws on Safari. Buffer all chunks until `tts_end`, then play via Blob URL.
- **AudioContext unlock**: Guest language picker calls `new AudioContext(); ctx.resume()` on click to satisfy browser autoplay policy.
- **PCM encoding**: Web Audio captures Float32 [-1,1]. WhisperLiveKit expects Int16. Convert: `Math.max(-32768, Math.min(32767, float32 * 32768))`.
- **ElevenLabs API key required**: Set `ELEVENLABS_API_KEY` env var for TTS. No local model download needed.

---

## NLLB Language Codes

NLLB uses BCP-47-like codes, mapped in nllb/server.py:

| Our code | NLLB code |
|----------|-----------|
| en | eng_Latn |
| ja | jpn_Jpan |
| zh | zho_Hans |
| ko | kor_Hang |
| fr | fra_Latn |

---

## Project Structure

```
brivva/
├── CLAUDE.md                      ← project instructions
├── BENCHMARK.md                   ← benchmark dashboard spec
├── docs.md                        ← this file
├── docker-compose.yml             ← orchestrates all 4 services (GPU)
├── .dockerignore
├── init.sh                        ← local dev: starts all services (Mac MPS)
├── server-rs/                     ← Rust axum WebSocket server
│   ├── Dockerfile
│   ├── Cargo.toml
│   └── src/
│       ├── main.rs                ← entry, router, state
│       ├── types.rs               ← Lang, Room, Guest, ServerMsg, Rooms
│       ├── pipeline.rs            ← STT → Translate → TTS pipeline
│       └── room/
│           ├── mod.rs
│           └── handler.rs         ← WebSocket host/guest handlers
├── stt-wrapper/                   ← STT proxy (CF Nova-3 via AI Gateway)
│   ├── Dockerfile
│   └── server.py                  ← asyncio WebSocket proxy
├── nllb/                          ← NLLB translation server
│   ├── Dockerfile
│   └── server.py                  ← FastAPI, POST /translate
├── kokoro/                        ← Kokoro TTS (legacy, replaced by ElevenLabs)
├── frontend/                      ← React + TypeScript + Vite
│   └── src/
│       ├── pages/HostPage.tsx     ← host UI + latency dashboard
│       ├── pages/GuestPage.tsx    ← guest UI + audio playback
│       ├── components/
│       │   ├── LatencyDashboard.tsx
│       │   └── PipelineAnalysis.tsx
│       ├── hooks/
│       │   ├── useHostRoom.ts
│       │   ├── useGuestRoom.ts
│       │   └── useTimings.ts
│       ├── lib/
│       │   ├── RoomSocket.ts      ← WebSocket client
│       │   └── TtsPlayer.ts       ← TTS audio queue + playback
│       └── state/
│           ├── host/              ← host reducer + message handler
│           └── guest/             ← guest reducer + message handler
└── worker/                        ← CF Worker (v1/v2, legacy)
```

---

## Historical: v2 Architecture (CF Workers + Durable Objects)

```
Host Browser → CF Worker → RoomDO (Durable Object, pinned to host's DC)
  → Nova-3 (Deepgram, CF AI Gateway or direct API)
  → M2M100-1.2B (CF Workers AI)
  → Kokoro (self-hosted via cloudflared tunnel)
  → Broadcast to guests per language group
```

Replaced by v3 because:
- CF Workers AI M2M100 = 394-870ms + network overhead
- Deepgram API = extra network hop to US
- Durable Objects add complexity (DO pinning, isolate eviction)
- All services on localhost eliminates ~170-350ms network overhead per utterance

## Historical: v1 Architecture (single-user)

```
Browser mic → CF Worker → Nova-3 (CF AI Gateway) → M2M100 (CF Workers AI)
  → Kokoro (self-hosted) → WebSocket binary → Browser Blob URL playback
```

Single-user EN→JA translation demo. Still deployed at `/api/realtime`.

---

## Secrets & Infrastructure

**v3 env vars (docker-compose / init.sh):**
```
CF_ACCOUNT_ID       = 80a55132ae169d5b282ccf505bc66bf7
CF_API_TOKEN        = (CF AI Gateway auth for stt-wrapper)
ELEVENLABS_API_KEY  = (ElevenLabs TTS API key)
```

**Cloudflare (legacy v1/v2):**
```
CF_AI_GATEWAY_ID = default
DEEPGRAM_API_KEY = (wrangler secret, optional)
```

**Cloudflared tunnel:**
- Config: `~/.cloudflared/brivva-kokoro.yml`
- `brivva-server.milliytechnology.org` → localhost:3000

**Frontend:**
- `VITE_WORKER_URL=https://brivva-server.milliytechnology.org`
- Deployed on Cloudflare Pages: https://brivva.pages.dev
