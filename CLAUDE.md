# Brivva Real-Time Translation Prototype

## Purpose

Demo prototype for Brivva interview — showing a real-time voice translation pipeline with rooms.
I'm the top candidate out of 15 and they want to proceed to a paid technical test.

## Current Status (Mar 15 2026)

**v3 — Self-hosted Rust pipeline, WORKING.**

- **Frontend:** https://brivva.pages.dev (Cloudflare Pages)
- **Backend:** Rust axum server via cloudflared tunnel at `brivva-server.milliytechnology.org`
- **Repo:** https://github.com/TyposBro/brivva (private)
- Host speaks (EN) → guests pick EN/JA/ZH → each gets translated ElevenLabs audio
- Docker works on both macOS (CPU) and Linux (GPU override)
- Measured latency: Avg 862ms, Best 653ms, 2.9x gap to 300ms target

**v2** (CF Workers + Durable Objects) still deployed at worker URL but superseded.
**v1** still deployed at `/api/realtime` (EN→JA, single-user, untouched).

---

## v3 Architecture (current)

```
Host Browser (/host)
├── Record audio from mic (ScriptProcessorNode, PCM linear16 @ 16kHz)
├── WebSocket → wss://brivva-server.milliytechnology.org/api/room?role=host&sourceLang=en
└── See room code + guest counts + live transcript + latency dashboard

server-rs (Rust, axum, DashMap rooms, tokio tasks)
├── STT Wrapper (stt-wrapper:8766/asr, Python asyncio)
│   - Proxies audio to CF Nova-3 via CF AI Gateway
│   - Emits clean { type: "interim"/"final", text } events
│   - Handles: silence stripping, sentence boundary detection, dedup
│   - Env: CF_ACCOUNT_ID, CF_API_TOKEN, STT_LANGUAGE
├── NLLB Translation (nllb:8000/translate, nllb-200-distilled-600M)
│   - POST /translate { text, source_lang, target_lang }
│   - 82–164ms on GPU, slower on CPU
│   - Parallel tokio::spawn per active language
├── ElevenLabs TTS (API, eleven_flash_v2_5)
│   - POST /v1/text-to-speech/{voice_id}/stream → streaming MP3
│   - 548–1,440ms depending on text length
│   - Voices: Sarah (EN), Lily (JA), Alice (ZH), Jessica (KO)
│   - Env: ELEVENLABS_API_KEY
└── Fan-out: translate once per language, broadcast to all guests in group

Guest Browser (/room/:id)
├── Join room with code, pick language (EN/JA/ZH)
├── WebSocket → receive translated audio + subtitles
└── Blob URL playback (FIFO queue, no overlap)
```

### WebSocket Protocol (v3)

Connection via query params (no JSON handshake):
- Host: `ws://host/api/room?role=host&sourceLang=en`
- Guest: `ws://host/api/room?role=guest&roomId=ABC123&lang=ja`

**Server → Host:** `room:created`, `room:guest_count`, `interim`, `final`, `translation`, `tts_end`
**Server → Guest:** `room:joined`, `interim`, `final`, `translation`, `tts_start`, [MP3 chunks], `tts_end`, `room:closed`

### Key files

| File | Purpose |
|------|---------|
| `server-rs/src/main.rs` | Entry, router, DashMap state |
| `server-rs/src/types.rs` | Lang, Room, Guest, ServerMsg, voice_id() |
| `server-rs/src/pipeline.rs` | STT → Translate → TTS pipeline (ElevenLabs) |
| `server-rs/src/room/handler.rs` | WebSocket host/guest handlers |
| `stt-wrapper/server.py` | Clean STT proxy (CF Nova-3 → interim/final) |
| `nllb/server.py` | NLLB FastAPI translation server |

---

## Tech Stack

- **Frontend:** React + TypeScript + Vite (Cloudflare Pages)
- **Backend:** Rust axum WebSocket server
- **STT:** CF Nova-3 via stt-wrapper (CF AI Gateway, streaming)
- **Translation:** NLLB-200-distilled-600M (self-hosted FastAPI, 82–164ms GPU)
- **TTS:** ElevenLabs flash_v2_5 (API, 32 languages, streaming MP3)
- **Rooms:** DashMap + mpsc channels (in-memory, per-process)
- **Tunnel:** cloudflared (routes brivva-server.milliytechnology.org → localhost:3000)
- **Docker:** Unified for macOS + Linux (GPU override via docker-compose.gpu.yml)

## Running

### Docker (works on macOS and Linux)

```bash
docker compose up --build                    # CPU (works everywhere)
# OR with NVIDIA GPU:
docker compose -f docker-compose.yml -f docker-compose.gpu.yml up --build
```

### Deploy frontend

```bash
cd frontend && npm run deploy
```

### Env vars required (.env file)

```
CF_ACCOUNT_ID=...        # CF AI Gateway for stt-wrapper
CF_API_TOKEN=...         # CF AI Gateway auth
ELEVENLABS_API_KEY=...   # ElevenLabs TTS
```

See `README.md` for full setup instructions (tunnel credentials, NixOS, AWS).

---

## About Brivva (from interview Mar 10)

- Real-time multilingual live commerce platform
- Voice translation + lip-sync for live streams
- Distribute to TikTok, Naver, Instagram, Rakuten simultaneously
- Founders have a previous exit
- Seed round closing April 2025
- 10 paying customers ($30-80K contracts each)
- Stack: Rust backend, React/TS frontend, AWS
- Target latency: <300ms end-to-end
- They acknowledged I need ~3 months Rust ramp-up and are fine with it

## Why This Matters for the Interview

Brivva's listed pipeline: Whisper STT → Context NMT → Emotive TTS → Wav2Lip
My improvements across v1→v3:

1. **Streaming STT** (CF Nova-3 via stt-wrapper) over batch Whisper — real-time interims
2. **NLLB self-hosted** — no cloud dependency, 200 languages, fast seq2seq
3. **ElevenLabs TTS** — 32-language support, natural voices, no local GPU needed
4. **Rust axum server** — matches Brivva's stack, replaces CF Workers
5. **Rooms + multi-language fan-out** — directly maps to live commerce product
6. **Dockerized** — unified macOS/Linux, GPU override for production
7. **API-based STT + TTS** — only NLLB needs local GPU, simplifies deployment

## Key Technical Decisions & Bug Fixes

- **CF Nova-3 over WhisperLiveKit** — API-based, no local GPU needed for STT
- **ElevenLabs over Kokoro** — 32 languages, no model downloads, no GPU for TTS
- **stt-wrapper** — clean proxy that handles silence stripping, sentence boundaries, dedup
- **STT retry loop** — stt-wrapper may still be starting; 10 retries, 3s apart
- **TTS Blob URL** — MSE addSourceBuffer("audio/mpeg") throws on Safari; Blob URL works everywhere
- **AudioContext unlock** — call ctx.resume() on user gesture before first audio.play()
- **Unified Docker** — python:3.10-slim base for NLLB (ARM + x86), GPU override via docker-compose.gpu.yml

## Project Structure

```
brivva/
├── docker-compose.yml             3 services (server, stt-wrapper, nllb)
├── docker-compose.gpu.yml         GPU override (NVIDIA runtime + CUDA Dockerfile)
├── .env                           API keys (CF_ACCOUNT_ID, CF_API_TOKEN, ELEVENLABS_API_KEY)
├── server-rs/                     Rust axum WebSocket server
│   ├── Dockerfile
│   └── src/
│       ├── main.rs                Entry, router, state
│       ├── types.rs               Lang, Room, Guest, ServerMsg, voice_id()
│       ├── pipeline.rs            STT → Translate → TTS pipeline
│       └── room/handler.rs        WebSocket host/guest handlers
├── stt-wrapper/                   STT proxy (CF Nova-3 via AI Gateway)
│   ├── Dockerfile
│   └── server.py                  asyncio WebSocket, interim/final events
├── nllb/                          NLLB translation server
│   ├── Dockerfile                 CPU (python:3.10-slim, works everywhere)
│   ├── Dockerfile.gpu             GPU (nvidia/cuda:12.4.1, Linux only)
│   └── server.py                  FastAPI, POST /translate
├── frontend/                      React + TypeScript + Vite
│   └── src/
│       ├── pages/                 HostPage, GuestPage, HomePage
│       ├── components/            LatencyDashboard, PipelineAnalysis
│       ├── hooks/                 useHostRoom, useGuestRoom, useTimings
│       ├── lib/                   RoomSocket, TtsPlayer
│       └── state/                 Host/guest reducers + message handlers
├── worker/                        CF Worker (v1/v2 legacy)
├── CLAUDE.md                      This file
├── BENCHMARK.md                   Benchmark dashboard spec
├── README.md                      Setup + deployment guide
└── docs.md                        Full technical documentation
```

## Rules

- Ship fast. This is a demo, not production code.
- Host language: English (testing) or Korean (Brivva's K-Beauty use case)
- Guest languages: EN, JA, ZH (Brivva's priority markets)
- No auth, no persistent storage — in-memory rooms are fine
- Room system should work with at least 3 simultaneous connections for demo
