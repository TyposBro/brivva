# Brivva Real-Time Translation Prototype

## Purpose

Demo prototype for Brivva technical interview. Real-time voice translation + lip-sync for live commerce.
I'm the top candidate out of 15. In-person meeting Friday Mar 21, 12PM at Yeongdeungpo Times Square coffee shop with Simon and CTO.

## Current Status (Mar 17 2026)

**v3 — WORKING and deployed.** Fully self-hosted Rust pipeline:

- **Frontend:** https://brivva.pages.dev (Cloudflare Pages)
- **Backend:** Rust axum server (server-rs :3000) via cloudflared tunnel
- **Tunnel:** brivva-server.milliytechnology.org → localhost:3000
- **Repo:** https://github.com/TyposBro/brivva (private)
- Host speaks (EN or KO) → guests pick EN/JA/ZH → each gets translated ElevenLabs audio
- Rooms backed by DashMap (concurrent hash map, per-shard locking)
- STT: CF Nova-3 via stt-wrapper (streaming interims + finals)
- Translation: NLLB-200-distilled-600M (self-hosted, 82-164ms GPU)
- TTS: ElevenLabs eleven_flash_v2_5 (API, 548-1440ms, 32 languages)
- Dockerized: 3 containers (server-rs, stt-wrapper, nllb)

**Next: Add MuseTalk lip-sync as 4th container (see Lip-Sync Integration section below)**

## Architecture (v3)

```
Host Browser (/host)
  → PCM linear16 @ 16kHz → WebSocket → server-rs (Rust/axum :3000)
  → stt-wrapper (:8766) → CF Nova-3 (streaming transcription)
  → NLLB (:8000) — per active language, parallel tokio::spawn
  → ElevenLabs TTS (API) — streaming MP3
  → WebSocket → Guest Browser (Blob URL playback)
```

### v4 Target Architecture (with lip-sync)

```
Host Browser (/host)
  → PCM audio → server-rs → STT → NLLB → ElevenLabs TTS
  → TTS audio (MP3) + host face image → MuseTalk (:8100)
  → MuseTalk returns lip-synced video frames
  → WebSocket → Guest Browser (video + audio playback)
```

| Service     | Port | Tech                             | Latency            | GPU |
| ----------- | ---- | -------------------------------- | ------------------ | --- |
| server-rs   | 3000 | Rust/axum, DashMap, mpsc         | orchestration      | No  |
| stt-wrapper | 8766 | Python asyncio, CF Nova-3 API    | streaming          | No  |
| NLLB        | 8000 | nllb-200-distilled-600M, FastAPI | 82-164ms           | Yes |
| ElevenLabs  | API  | eleven_flash_v2_5                | 548-1440ms         | No  |
| MuseTalk    | 8100 | MuseTalk v1.5, FastAPI           | ~33ms/frame @30fps | Yes |

## Lip-Sync Integration Plan (MuseTalk v1.5)

### Why MuseTalk

- **Real-time capable:** 30fps+ on V100, MIT license for code, commercial OK for model
- **Audio-driven:** Takes audio + face image → outputs lip-synced video frames (exactly what we need after TTS)
- **Multi-language:** Supports EN, JA, ZH audio input
- **256x256 face region:** Good enough for PIP window in live commerce stream
- **v1.5 improvements:** GAN + perceptual + sync loss for better quality, training code open-sourced

### Alternatives Considered

| Tool          | Type                | Real-time? | Why Not                               |
| ------------- | ------------------- | ---------- | ------------------------------------- |
| Wav2Lip       | Lips only           | Fast       | Uncanny, frozen face                  |
| LivePortrait  | Expression transfer | ~80ms      | Needs driving video, not audio        |
| Sync Labs API | Cloud lip-sync      | sub-200ms  | API dependency, expensive at scale    |
| InfiniteTalk  | Full body + face    | Batch only | Not real-time, can't self-host easily |

### Dockerization Plan

```dockerfile
# musetalk/Dockerfile
FROM nvidia/cuda:12.4.1-runtime-ubuntu22.04

# Python 3.10 + PyTorch + CUDA
# mmcv, mmpose, mmdet (MMLab ecosystem)
# Model weights downloaded at build time
# FastAPI wrapper exposing REST API

EXPOSE 8100
```

### API Design (musetalk/server.py)

```
POST /lipsync
  Body: { "audio_base64": "...", "face_image_base64": "..." }
  Response: { "frames_base64": ["frame1", "frame2", ...], "fps": 25, "lipsync_ms": 342 }

POST /lipsync/stream  (stretch goal)
  Body: { "audio_base64": "...", "face_image_base64": "..." }
  Response: streaming MJPEG or raw RGB frames

GET /health
  Response: { "status": "healthy", "model": "musetalk_v1.5", "device": "cuda" }
```

### Integration into server-rs Pipeline

```
Current: ... → ElevenLabs TTS (MP3 audio) → stream to guests
With lip-sync: ... → ElevenLabs TTS (MP3 audio)
  → server-rs sends audio + host face to MuseTalk :8100
  → MuseTalk returns lip-synced video frames
  → server-rs streams video frames + audio to guests
```

### Host Face Capture

- Host's webcam captures a reference face image on room creation
- Sent to server-rs as a single frame (not streaming video)
- Stored in Room state, reused for all lip-sync calls in that room
- Frontend: capture from `<video>` element using canvas.toDataURL()

### Guest Playback Changes

- Currently: audio only (Blob URL MP3 playback)
- With lip-sync: video + audio (MJPEG or canvas rendering + audio sync)
- Stretch: `<video>` element with MediaSource Extensions for synchronized playback

### GPU Requirements

- MuseTalk inference: ~2-4GB VRAM (fp16)
- NLLB: ~2GB VRAM
- Total: ~4-6GB VRAM — fits on RTX 4060 (8GB) or A10G (24GB)
- RTX 3050 Ti 4GB can run MuseTalk alone but tight with NLLB

### Docker Compose (v4 — 4 containers)

```yaml
services:
  server-rs:
    build: ./server-rs
    ports: ["3000:3000"]
    depends_on: [stt-wrapper, nllb, musetalk]

  stt-wrapper:
    build: ./stt-wrapper
    ports: ["8766:8766"]

  nllb:
    build: ./nllb
    ports: ["8000:8000"]
    deploy:
      resources:
        reservations:
          devices:
            - capabilities: [gpu]

  musetalk:
    build: ./musetalk
    ports: ["8100:8100"]
    deploy:
      resources:
        reservations:
          devices:
            - capabilities: [gpu]
```

### Build Order

1. **Dockerize MuseTalk standalone** — get inference working in container with GPU
2. **Add FastAPI wrapper** — POST /lipsync endpoint, health check
3. **Test standalone** — curl with sample audio + face → get frames back
4. **Add to docker-compose** — 4th container alongside existing 3
5. **Integrate into pipeline.rs** — after TTS, call MuseTalk, stream frames
6. **Update frontend** — GuestPage renders video frames + plays audio
7. **Benchmark** — measure added latency from lip-sync step

### Risks

- **MuseTalk dependency hell:** mmcv/mmpose/mmdet have strict version requirements. Docker isolates this.
- **GPU memory contention:** NLLB + MuseTalk sharing GPU. May need to sequence, not parallelize.
- **Latency increase:** MuseTalk adds ~33ms/frame at 30fps. For 2-second audio = ~60 frames = ~2s processing. May need to pipeline (process frames as audio streams in).
- **Video streaming complexity:** Switching from audio-only to video+audio changes the entire guest playback. May keep audio-only as default, video as opt-in.

### Deadline

- **Thursday 6pm** — stop working on lip-sync regardless of state
- **If working:** demo with video + audio Friday
- **If not working:** demo with audio only Friday (still impressive), mention lip-sync as scoped and in progress

## Key Technical Decisions

### Why Rust axum over CF Workers (v2 → v3 migration)

- CF Workers AI M2M100 = 394-870ms + network. NLLB localhost = 82-164ms
- Durable Objects add complexity (DO pinning, isolate eviction, no GPU)
- All services co-located eliminates ~170-350ms network overhead per utterance
- Persistent WebSocket connections (Workers have 30s idle timeout issues)
- GPU access for NLLB (Workers have no GPU)

### Why DashMap over HashMap+Mutex

- DashMap locks per-shard: concurrent room access without blocking all rooms
- HashMap+Mutex locks entire map: one slow room blocks all tokio tasks
- Real-world: 100 rooms = DashMap handles concurrent joins/leaves; Mutex serializes them

### Why CF Nova-3 (via stt-wrapper) over Whisper

- Streaming (interims while speaking) vs batch-only
- Better accuracy for short phrases (Whisper hallucinates on silence/ambiguous chunks)
- stt-wrapper proxies audio, returns clean { type: "interim"/"final", text } events
- No local GPU needed for STT

### Why NLLB over M2M100/LLM

- NLLB = Meta's successor to M2M100: better quality, 200 languages
- distilled-600M = half the size of M2M100-1.2B, faster inference
- Self-hosted = no CF Workers AI dependency, no network roundtrip
- Seq2seq = dedicated translation model, ~3x faster than LLM prompting

### Why ElevenLabs over Kokoro

- 32 languages with natural expressive voices
- Kokoro (82M params) sounds robotic, lacks emotional range
- API-based: no local GPU needed, no model download
- Tradeoff: API dependency + per-character cost

### Why MuseTalk over alternatives

- Audio-driven lip-sync (not expression transfer like LivePortrait)
- 30fps+ real-time inference on V100
- MIT license for code, commercial OK for model
- Multi-language audio support (EN, JA, ZH)
- v1.5 with improved quality (GAN + sync loss)
- Self-hostable: fits in Docker container with GPU

### Fan-out optimization

- Translate ONCE per language group, broadcast to all N guests
- 50 JA guests = 1 NLLB call + 1 ElevenLabs call + 1 MuseTalk call, not 50
- Each language runs in parallel via tokio::spawn + tokio::join!

### Blob URL playback over MSE

- MSE addSourceBuffer("audio/mpeg") throws on Safari
- Blob URL: buffer all chunks until tts_end, then play via Blob URL
- Works on every browser
- Tradeoff: slight delay (must wait for all chunks) vs MSE streaming

## WebSocket Protocol (v3)

Connection via query params (no JSON handshake):

- Host: `ws://host/api/room?role=host&sourceLang=en`
- Guest: `ws://host/api/room?role=guest&roomId=ABC123&lang=ja`

**Host → Server:** `[ArrayBuffer]` (PCM audio frames), `"host:end"` (close room)
**Server → Host:** room:created, room:guest_count, interim, final, translation, tts_end
**Server → Guest:** room:joined, interim, final, translation, tts_start, [MP3 chunks], tts_end, room:closed

### v4 Protocol Additions (with lip-sync)

**Host → Server:** (new) face image on room creation for lip-sync reference
**Server → Guest:** (new) video_start, [video frame chunks], video_end alongside tts_start/tts_end

## Bug Fixes Log

- **TTS queue freeze** — audio.play() Promise rejection left playing flag stuck true. Without .catch(), queue permanently frozen. Fix: .catch() calls advance() to move to next item.
- **AudioContext unlock** — Browser autoplay policy blocks audio.play() until user gesture. Guest language picker calls new AudioContext(); ctx.resume() on click.
- **URL.revokeObjectURL** — Must revoke blob URL after playback to prevent memory leaks. Each blob stays in memory until explicitly revoked.
- **Whisper hallucination** — Whisper generates random coherent text ("space whale", "sustainable shoe brand") on silence/ambiguous audio. Fix: switched to CF Nova-3 which handles silence correctly.
- **STT retry loop** — stt-wrapper may still be starting when server-rs boots. Retries connection 10 times, 3 seconds apart.
- **PCM encoding** — Web Audio captures Float32 [-1,1]. STT expects Int16. Convert: Math.max(-32768, Math.min(32767, float32 \* 32768)).

## Measured Latency (v3, self-hosted)

| Phase                              | Typical         | Notes                          |
| ---------------------------------- | --------------- | ------------------------------ |
| STT (Nova-3 via stt-wrapper)       | streaming       | Interims arrive while speaking |
| Translation (NLLB GPU)             | 82-164ms        | Warm, per language             |
| TTS (ElevenLabs)                   | 548-1440ms      | Depends on text length         |
| Lip-sync (MuseTalk, planned)       | ~33ms/frame     | 30fps on V100, TBD on RTX 4060 |
| Overhead (routing)                 | ~11ms           | localhost, no network hops     |
| **Total from utterance finalized** | **~650-1600ms** | Without lip-sync               |
| **Target**                         | **<300ms**      | Gap: ~2-5x                     |

## Rust Server Key Concepts

- **AppState:** Arc<AppState> holds DashMap<String, Room> (rooms) + reqwest::Client (shared HTTP client)
- **Room:** host_tx (mpsc sender to host), guests DashMap<String, Guest>, source_lang
- **Guest:** tx (mpsc sender), lang, id
- **mpsc channels:** Each WebSocket connection gets a (tx, rx) pair. tx stored in Room, rx drives the WebSocket send loop
- **tokio::spawn:** Used for parallel per-language translation+TTS. One task per active language group
- **Pipeline flow:** handle_host_message → pipeline::process_utterance → spawn per-lang → translate → tts → (lipsync) → broadcast

## Frontend Architecture (refactored, clean)

- `src/lib/AudioPipeline.ts` — mic capture, AudioContext, Float32→Int16 conversion
- `src/lib/RoomSocket.ts` — WebSocket connect, sendAudio, sendJson, routeMessage
- `src/lib/TtsPlayer.ts` — Blob URL playback queue (startReceiving → addChunk → finishReceiving → advance)
- `src/hooks/hostReducer.ts` — pure function, all state transitions, zero side effects
- `src/hooks/useHostRoom.ts` — thin orchestrator: WS messages → dispatch, uses AudioPipeline + RoomSocket
- `src/hooks/useTimings.ts` — per-utterance stopwatch (startTimer → recordSplit → finalize)
- `src/components/LatencyDashboard.tsx` — live per-utterance stacked bars
- `src/components/PipelineAnalysis.tsx` — static pipeline comparison

## Docker Services

```yaml
# docker-compose.yml — 3 containers (current), 4 with lip-sync
server-rs: Rust axum, port 3000, no GPU
stt-wrapper: Python asyncio, port 8766, no GPU (CF Nova-3 API proxy)
nllb: FastAPI + nllb-200-distilled-600M, port 8000, optional GPU
musetalk: FastAPI + MuseTalk v1.5, port 8100, GPU required (planned)
```

## Project Structure

```
brivva/
├── server-rs/                     Rust axum WebSocket server
│   ├── Dockerfile
│   └── src/
│       ├── main.rs                Entry, router, AppState
│       ├── types.rs               Lang, Room, Guest, ServerMsg
│       ├── pipeline.rs            STT → Translate → TTS → (Lip-sync) pipeline
│       └── room/handler.rs        WebSocket host/guest handlers
├── stt-wrapper/                   STT proxy (CF Nova-3)
│   └── server.py                  asyncio WebSocket proxy
├── nllb/                          NLLB translation server
│   └── server.py                  FastAPI, POST /translate
├── musetalk/                      MuseTalk lip-sync server (planned)
│   ├── Dockerfile
│   └── server.py                  FastAPI, POST /lipsync
├── frontend/                      React 19 + TypeScript + Vite
│   └── src/
│       ├── pages/                 HostPage, GuestPage, HomePage
│       ├── components/            LatencyDashboard, PipelineAnalysis
│       ├── hooks/                 useHostRoom, useGuestRoom, useTimings, hostReducer
│       ├── lib/                   AudioPipeline, RoomSocket, TtsPlayer
│       └── state/                 host/guest reducers + message handlers
├── brivva-frames/                 Rust frame extraction CLI (built, working)
├── docker-compose.yml             3 services (4 with lip-sync)
├── .env                           API keys
└── CLAUDE.md                      ← you are here
```

## Other Rust Projects Built

### brivva-frames (completed)

Real-time video frame extraction + face cropping pipeline in Rust.

- Spawns FFmpeg, reads raw RGB frames from stdout pipe
- Center-crops face region to 256×256 (MuseTalk input size)
- Parallel batch processing with rayon, streaming with mpsc channels
- Live preview window with minifb (side-by-side: full frame + cropped face)
- Performance: 640x480 @28fps, 5.86ms avg crop latency
- Demonstrates: ownership, borrowing, channels, parallel iterators, process spawning, Drop

## About Brivva

- Real-time multilingual live commerce platform
- Voice translation + lip-sync for live streams
- Distribute to TikTok, Naver, Instagram, Rakuten simultaneously
- Founders have previous exit, seed round closing April
- 10 paying customers ($30-80K contracts)
- Stack: Rust backend, React/TS frontend, AWS
- Target: <300ms e2e latency
- Salary discussed: ₩80-100M + equity, remote OK (KST)
- Meeting: Friday Mar 21 12PM, Yeongdeungpo Times Square coffee shop
