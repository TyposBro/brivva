# Brivva Real-Time Translation Prototype

## Purpose

Demo prototype for Brivva interview — showing a real-time voice translation pipeline with rooms.
I'm the top candidate out of 15 and they want to proceed to a paid technical test.

## Current Status (Mar 15 2026)

**v3 — Self-hosted Rust pipeline, WORKING.**

- **Frontend:** https://brivva.pages.dev (Cloudflare Pages)
- **Backend:** Rust axum server via cloudflared tunnel at `brivva-server.milliytechnology.org`
- **Repo:** https://github.com/TyposBro/brivva (private)
- All ML services self-hosted on localhost (no cloud API dependencies)
- Dockerized with NVIDIA GPU support for Linux/ECS deployment
- Host speaks (EN) → guests pick EN/JA/ZH → each gets translated Kokoro audio

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
│   - Proxies audio to WhisperLiveKit, emits clean interim/final events
│   - Handles: silence stripping, sentence boundary detection, dedup
│   └── WhisperLiveKit (whisper-stt:8765, faster-whisper base.en)
│       - Streaming WebSocket, --no-vac --pcm-input
├── NLLB Translation (nllb:8000/translate, nllb-200-distilled-600M)
│   - POST /translate { text, source_lang, target_lang }
│   - Parallel tokio::spawn per active language
├── Kokoro TTS (kokoro:8880/v1/audio/speech, kokoro-v1_0 82M)
│   - POST /v1/audio/speech → streaming MP3 response
│   - Voices: af_bella (EN), jf_alpha (JA), zf_xiaobei (ZH)
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
| `server-rs/src/types.rs` | Lang, Room, Guest, ServerMsg, Rooms |
| `server-rs/src/pipeline.rs` | STT → Translate → TTS pipeline |
| `server-rs/src/room/handler.rs` | WebSocket host/guest handlers |
| `stt-wrapper/server.py` | Clean STT proxy (WhisperLiveKit → interim/final) |
| `nllb/server.py` | NLLB FastAPI translation server |

---

## Tech Stack

- **Frontend:** React + TypeScript + Vite (Cloudflare Pages)
- **Backend:** Rust axum WebSocket server
- **STT:** WhisperLiveKit (mlx-whisper base.en, streaming)
- **Translation:** NLLB-200-distilled-600M (self-hosted FastAPI)
- **TTS:** Kokoro 82M (self-hosted Kokoro-FastAPI, MPS/CUDA)
- **Rooms:** DashMap + mpsc channels (in-memory, per-process)
- **Tunnel:** cloudflared (routes brivva-server.milliytechnology.org → localhost:3000)
- **Docker:** NVIDIA CUDA 12.4 base images for GPU deployment

## Running

### Local (Mac M1/M2 — MPS)

```bash
bash init.sh    # starts all 5 services + cloudflared tunnel
```

### Docker (Linux with NVIDIA GPU)

```bash
git clone https://github.com/remsky/Kokoro-FastAPI kokoro
docker compose up --build
cloudflared tunnel --config ~/.cloudflared/brivva-kokoro.yml run
```

### Deploy frontend

```bash
cd frontend && npm run deploy
```

See `README.md` for full setup instructions (venvs, tunnel credentials, NixOS, AWS).

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

1. **Streaming STT** (WhisperLiveKit) over batch Whisper — real-time interims
2. **NLLB self-hosted** — no cloud dependency, 200 languages, fast seq2seq
3. **Kokoro TTS** over Emotive TTS (3B) — 82M params, open-source, self-hostable
4. **Rust axum server** — matches Brivva's stack, replaces CF Workers
5. **Rooms + multi-language fan-out** — directly maps to live commerce product
6. **Dockerized with GPU** — ready for ECS deployment
7. **Self-hosted pipeline** — eliminates cloud API latency (~170-350ms saved)

## Key Technical Decisions & Bug Fixes

- **base.en over large-v3-turbo** — turbo caused 164s lag on M1 Pro; base.en streams in real-time
- **--no-vac required** — WhisperLiveKit's Voice Activity Controller blocks audio from transcriber
- **"(silence)" bug** — WhisperLiveKit appends "(silence)" to line text; fixed by stripping + emitting once per line index
- **STT retry loop** — WhisperLiveKit may still be loading when server starts; 10 retries, 3s apart
- **TTS Blob URL** — MSE addSourceBuffer("audio/mpeg") throws on Safari; Blob URL works everywhere
- **AudioContext unlock** — call ctx.resume() on user gesture before first audio.play()
- **UniDic for Japanese** — `python -m unidic download` (526MB), checked by init.sh
- **Dual STT detection** — track both buffer_transcription transitions AND lines[].text changes (--no-vac mode)

## Project Structure

```
brivva/
├── docker-compose.yml             All 4 services (NVIDIA GPU)
├── init.sh                        Local dev: starts everything (Mac MPS)
├── server-rs/                     Rust axum WebSocket server
│   ├── Dockerfile
│   └── src/
│       ├── main.rs                Entry, router, state
│       ├── types.rs               Lang, Room, Guest, ServerMsg
│       ├── pipeline.rs            STT → Translate → TTS pipeline
│       └── room/handler.rs        WebSocket host/guest handlers
├── stt-wrapper/                   Clean STT WebSocket proxy
│   ├── Dockerfile
│   └── server.py                  asyncio websockets, interim/final events
├── nllb/                          NLLB translation server
│   ├── Dockerfile
│   └── server.py                  FastAPI, POST /translate
├── whisper-stt/                   WhisperLiveKit STT
│   └── Dockerfile
├── kokoro/                        Kokoro TTS (clone from remsky/Kokoro-FastAPI)
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
- kokoro/ is a separate repo (remsky/Kokoro-FastAPI) — clone separately, not committed to brivva
