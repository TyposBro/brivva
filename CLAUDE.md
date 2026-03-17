# Brivva Real-Time Translation Prototype

## Purpose

Demo prototype for Brivva technical interview. Real-time voice translation + lip-sync for live commerce.
I'm the top candidate out of 15. In-person meeting Friday Mar 21, 12PM at Yeongdeungpo Times Square coffee shop with Simon and CTO.

## Current Status (Mar 17 2026)

**v4 — IN PROGRESS.** Lip-sync integration into fully self-hosted Rust pipeline:

- **Frontend:** https://brivva.pages.dev (Cloudflare Pages)
- **Backend:** Rust axum server (server-rs :3000) via cloudflared tunnel
- **Tunnel:** brivva-server.milliytechnology.org → localhost:3000
- **Repo:** https://github.com/TyposBro/brivva (private)
- Host speaks (EN or KO) → guests pick EN/JA/ZH → each gets translated ElevenLabs audio + lip-synced video
- Rooms backed by DashMap (concurrent hash map, per-shard locking)
- STT: CF Nova-3 via stt-wrapper (streaming interims + finals)
- Translation: NLLB-200-distilled-600M (self-hosted, 82-164ms GPU)
- TTS: ElevenLabs eleven_flash_v2_5 (API, 548-1440ms, 32 languages)
- Lip-sync: MuseTalk v1.5 (self-hosted, GPU, ~33ms/frame @25fps)
- Dockerized: 4 containers (server-rs, stt-wrapper, nllb, musetalk)

### What's Done (v4)

- [x] MuseTalk Dockerized — CUDA 11.8 + PyTorch 2.0.1 + mmcv prebuilt wheels + FastAPI wrapper
- [x] Single-call API: POST /lipsync (face frame + audio → lip-synced frames)
- [x] Full pipeline working end-to-end: STT → NLLB → TTS → MuseTalk → guest receives audio + video
- [x] Host streams webcam at 30fps, server stores latest_face for lip-sync
- [x] Synced delivery: TTS audio buffered, sent to guest only after MuseTalk completes (audio+video together)
- [x] Fallback: if MuseTalk fails, audio-only sent to guest
- [x] All 4 Docker containers running, frontend deployed to CF Pages with tunnel
- [x] Numpy-based face detection (no file I/O) for MuseTalk preprocessing

### Performance Problem (v4)

**MuseTalk is ~5x too slow for real-time on RTX 4060:**

Measured (3.7s audio clip, 92 frames @25fps):
- Face detection + VAE encode (prep): **1026ms** per call
- UNet inference: **~177ms/frame** (92 frames in ~16.3s)
- Total: **17.3s** for 3.7s of audio = **4.7x slower than real-time**
- Full pipeline: STT (~1s) + NLLB (~1.5s) + TTS (~0.7s) + MuseTalk (~17s) = **~20s total latency**

Root causes:
- Per-frame UNet inference without batching (~177ms/frame on RTX 4060)
- Face detection (mmpose + face_alignment) runs per-call (~500ms)
- VAE encode/decode per frame
- Python overhead, no CUDA stream pipelining

Possible optimizations:
- **Batch UNet inference** — process N frames at once (limited by VRAM)
- **Cache face detection** — reuse bbox across calls if face hasn't moved much
- **TensorRT/ONNX** — export UNet to TensorRT for ~3-5x speedup
- **Reduce frame count** — generate at 15fps instead of 25fps, interpolate on client
- **fp16 already enabled** — but torch.compile() or flash attention could help

### What's Not Working

- Guest video playback not rendering lip-synced frames properly (VideoPlayer queue issue)
- Audio plays but video frames may not be displaying on guest canvas
- 30fps webcam streaming from host to server works but is bandwidth-heavy (~450KB/s)

## Architecture (v4)

```
Host Browser (/host)
  → Webcam streams face frames (256x256 JPEG, ~5fps) → face:frame → server-rs
  → server-rs stores latest_face in Room, forwards face:frame to guests (live video)
  → PCM linear16 @ 16kHz → WebSocket → server-rs (Rust/axum :3000)
  → stt-wrapper (:8766) → CF Nova-3 (streaming transcription)
  → NLLB (:8000) — per active language, parallel tokio::spawn
  → ElevenLabs TTS (API) — streaming MP3 (buffered + streamed to guests)
  → Buffered MP3 + latest face frame → MuseTalk (:8100) /lipsync → JPEG frames
  → video_start → video_frame[] → video_end → Guest Browser (canvas rendering)

Host sees: mirrored webcam preview (local <video> element)
Guests see: live host face on <canvas> (5fps) + lip-synced face during translations + audio via Blob URL
```

| Service     | Port | Tech                             | Latency            | GPU  | CUDA |
| ----------- | ---- | -------------------------------- | ------------------ | ---- | ---- |
| server-rs   | 3000 | Rust/axum, DashMap, mpsc         | orchestration      | No   | —    |
| stt-wrapper | 8766 | Python asyncio, CF Nova-3 API    | streaming          | No   | —    |
| NLLB        | 8000 | nllb-200-distilled-600M, FastAPI | 82-164ms           | Yes  | 12.4 |
| ElevenLabs  | API  | eleven_flash_v2_5                | 548-1440ms         | No   | —    |
| MuseTalk    | 8100 | MuseTalk v1.5, FastAPI           | ~33ms/frame @25fps | Yes  | 11.8 |

## MuseTalk Integration (Implemented)

### Docker Container

- **Base:** nvidia/cuda:11.8.0-devel-ubuntu22.04 (must match mmcv prebuilt wheels)
- **PyTorch:** 2.0.1 + cu118 (MuseTalk's tested environment)
- **mmcv:** 2.0.1 installed via prebuilt wheel index (`-f https://download.openmmlab.com/mmcv/dist/cu118/torch2.0/index.html`) — NOT from source (source build fails)
- **MMLab stack:** mmengine + mmcv 2.0.1 + mmdet 3.1.0 + mmpose 1.1.0
- **MuseTalk:** cloned from GitHub at build time
- **Weights:** downloaded at build time via huggingface-cli (musetalkV15, sd-vae-ft-mse, whisper-tiny, dwpose, face-parse-bisent)

### API (musetalk/server.py)

```
POST /lipsync
  Body: { "audio_base64": "<base64 MP3>", "face_image_base64": "<base64 JPEG>" }
  Response: { "frames_base64": ["<JPEG>", ...], "fps": 25, "lipsync_ms": 1200 }
  → Detects face in image, crops 256x256, encodes VAE latent
  → Converts MP3→WAV via ffmpeg, extracts whisper features
  → UNet inference per audio frame (masked face latent + audio → lip movement)
  → VAE decode, blend back onto original image
  → Returns all frames as base64 JPEG array

GET /health
  Response: { "status": "healthy", "model": "musetalk_v1.5", "device": "cuda" }
```

No separate /prepare step — face is processed per-call so body movement stays current.

### Pipeline Integration (pipeline.rs)

```
Host streams face:frame at ~5fps → server stores latest_face in Room

do_tts_and_broadcast():
  1. Stream MP3 chunks to guests (existing audio flow, unchanged)
  2. Buffer all MP3 chunks in Vec<u8>
  3. After TTS complete, if latest_face exists:
     → Grab latest face frame from Room
     → POST { audio_base64, face_image_base64 } to MuseTalk /lipsync
     → Send video_start, video_frame[] (JSON with base64 JPEG), video_end to guests
```

### Host Face Streaming Flow

1. HostPage starts webcam on mount (512x512 request, getUserMedia)
2. Shows mirrored preview via `<video>` element (`transform: scaleX(-1)`)
3. When recording starts, captures face frames at ~5fps (200ms interval)
4. Each frame: center-crop to square → scale to 256x256 → JPEG base64
5. Sends `{ type: "face:frame", data: "<base64>" }` via WebSocket
6. handler.rs stores latest_face in Room, forwards face:frame to all guests
7. Guests render live host face on canvas via VideoPlayer.renderDirect()

### Guest Video Playback

- `VideoPlayer.ts` — two rendering modes:
  - `renderDirect(base64)` — immediate draw for live host face frames (~5fps)
  - Queue-based playback for lip-sync batches (video_start → addFrame → finishReceiving → render at 25fps)
- Live face frames show the host's actual camera feed (body movement, expressions)
- During translation, lip-synced frames override the live feed temporarily
- Audio plays via existing TtsPlayer (Blob URL), video renders on `<canvas>`

### Key Build Lesson: mmcv

- mmcv MUST be installed from prebuilt wheels, not pip/mim source install
- Prebuilt wheels only exist for specific CUDA+PyTorch combos
- CUDA 11.8 + PyTorch 2.0.1 has prebuilt mmcv 2.0.1 wheels
- CUDA 12.4 + PyTorch 2.5.1 does NOT — source build fails in Docker
- Each Docker container has its own CUDA runtime, so NLLB (cu124) and MuseTalk (cu118) don't conflict

## WebSocket Protocol (v4)

Connection via query params (no JSON handshake):

- Host: `ws://host/api/room?role=host&sourceLang=en`
- Guest: `ws://host/api/room?role=guest&roomId=ABC123&lang=ja`

**Host → Server:**
- `[ArrayBuffer]` — PCM audio frames
- `{ "type": "face:frame", "data": "<base64>" }` — live webcam frames (~5fps while recording)
- `"host:end"` — close room

**Server → Host:**
- room:created, room:guest_count, interim, final, translation, tts_end, video_end

**Server → Guest:**
- room:joined, interim, final, translation
- face:frame (live host face, forwarded from host → guests for real-time video)
- tts_start, [MP3 binary chunks], tts_end (audio)
- video_start, video_frame (lip-synced JPEG), video_end (lip-sync batch)
- room:closed

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

## Bug Fixes Log

- **TTS queue freeze** — audio.play() Promise rejection left playing flag stuck true. Without .catch(), queue permanently frozen. Fix: .catch() calls advance() to move to next item.
- **AudioContext unlock** — Browser autoplay policy blocks audio.play() until user gesture. Guest language picker calls new AudioContext(); ctx.resume() on click.
- **URL.revokeObjectURL** — Must revoke blob URL after playback to prevent memory leaks. Each blob stays in memory until explicitly revoked.
- **Whisper hallucination** — Whisper generates random coherent text ("space whale", "sustainable shoe brand") on silence/ambiguous audio. Fix: switched to CF Nova-3 which handles silence correctly.
- **STT retry loop** — stt-wrapper may still be starting when server-rs boots. Retries connection 10 times, 3 seconds apart.
- **PCM encoding** — Web Audio captures Float32 [-1,1]. STT expects Int16. Convert: Math.max(-32768, Math.min(32767, float32 \* 32768)).
- **mmcv build failure** — mmcv 2.0.1 has no prebuilt wheel for CUDA 12.4 + PyTorch 2.5.1. Fix: use CUDA 11.8 + PyTorch 2.0.1 base image with prebuilt wheel index URL.

## Measured Latency (v3, self-hosted)

| Phase                              | Measured        | Notes                              |
| ---------------------------------- | --------------- | ---------------------------------- |
| STT (Nova-3 via stt-wrapper)       | streaming       | Interims arrive while speaking     |
| Translation (NLLB CPU)             | 1537-2434ms     | Cold ~2.4s, warm ~400ms            |
| TTS (ElevenLabs)                   | 711ms           | Buffered, not streamed             |
| Lip-sync prep (face detect + VAE)  | 1026ms          | Per call, mmpose + face_alignment  |
| Lip-sync UNet (per frame)          | ~177ms          | 92 frames in ~16.3s on RTX 4060   |
| **Total (audio only, no lipsync)** | **~2.5-3.5s**   | STT + NLLB + TTS                   |
| **Total (with lip-sync)**          | **~20s**        | + ~17s MuseTalk (3.7s audio clip)  |
| **Target**                         | **<300ms**      | Gap: ~10x (audio), ~65x (video)    |

## Rust Server Key Concepts

- **AppState:** Arc<AppState> holds DashMap<String, Room> (rooms) + reqwest::Client (shared HTTP client)
- **Room:** host_tx (mpsc sender to host), guests DashMap<String, Guest>, source_lang, avatar_id (Option<String>)
- **Guest:** tx (mpsc sender), lang, id
- **mpsc channels:** Each WebSocket connection gets a (tx, rx) pair. tx stored in Room, rx drives the WebSocket send loop
- **tokio::spawn:** Used for parallel per-language translation+TTS+lipsync. One task per active language group
- **Pipeline flow:** handle_host_message → pipeline::process_utterance → spawn per-lang → translate → tts → lipsync → broadcast
- **prepare_avatar:** Called on face:image from host, POSTs to MuseTalk /prepare, stores avatar_id in Room

## Frontend Architecture

- `src/lib/AudioPipeline.ts` — mic capture, AudioContext, Float32→Int16 conversion
- `src/lib/RoomSocket.ts` — WebSocket connect, sendAudio, sendJson, routeMessage
- `src/lib/TtsPlayer.ts` — Blob URL playback queue (startReceiving → addChunk → finishReceiving → advance)
- `src/lib/VideoPlayer.ts` — Canvas-based JPEG frame renderer (startReceiving → addFrame → finishReceiving → render at FPS)
- `src/hooks/useHostRoom.ts` — orchestrator: WS messages → dispatch, AudioPipeline + RoomSocket + webcam capture
- `src/hooks/useGuestRoom.ts` — guest orchestrator: TtsPlayer + VideoPlayer + RoomSocket
- `src/hooks/useTimings.ts` — per-utterance stopwatch (startTimer → recordSplit → finalize)
- `src/state/host/reducer.ts` — pure function, all host state transitions
- `src/state/host/messageHandler.ts` — routes WS messages to host dispatch + stopwatch
- `src/state/guest/reducer.ts` — pure function, all guest state transitions
- `src/state/guest/messageHandler.ts` — routes WS messages to guest dispatch + TtsPlayer + VideoPlayer
- `src/components/LatencyDashboard.tsx` — live per-utterance stacked bars
- `src/components/PipelineAnalysis.tsx` — static pipeline comparison
- `src/pages/HostPage.tsx` — host UI with mirrored webcam preview, room code, audio recorder
- `src/pages/GuestPage.tsx` — guest UI with lip-synced video canvas, language picker, translations

## Docker Services

```yaml
# docker-compose.yml — 4 containers (v4)
server-rs: Rust axum, port 3000, no GPU, depends_on: [stt-wrapper, nllb, musetalk]
stt-wrapper: Python asyncio, port 8766, no GPU (CF Nova-3 API proxy)
nllb: FastAPI + nllb-200-distilled-600M, port 8000, GPU (CUDA 12.4)
musetalk: FastAPI + MuseTalk v1.5, port 8100, GPU required (CUDA 11.8)
# GPU: docker compose -f docker-compose.yml -f docker-compose.gpu.yml up --build
```

## Project Structure

```
brivva/
├── server-rs/                     Rust axum WebSocket server
│   ├── Dockerfile
│   ├── Cargo.toml                 deps: axum, dashmap, reqwest, tokio, serde, base64
│   └── src/
│       ├── main.rs                Entry, router, AppState
│       ├── types.rs               Lang, Room (+ avatar_id), Guest, ServerMsg (+ Video*)
│       ├── pipeline.rs            STT → Translate → TTS → Lip-sync pipeline
│       └── room/handler.rs        WebSocket host/guest handlers (+ face:image parsing)
├── stt-wrapper/                   STT proxy (CF Nova-3)
│   └── server.py                  asyncio WebSocket proxy
├── nllb/                          NLLB translation server
│   ├── Dockerfile / Dockerfile.gpu
│   └── server.py                  FastAPI, POST /translate
├── musetalk/                      MuseTalk lip-sync server
│   ├── Dockerfile                 CUDA 11.8 + PyTorch 2.0.1 + mmcv prebuilt
│   ├── requirements.txt           FastAPI, diffusers, transformers, opencv, librosa
│   ├── download_weights.sh        HuggingFace weight downloader (build-time)
│   └── server.py                  FastAPI, POST /prepare + POST /lipsync + GET /health
├── frontend/                      React 19 + TypeScript + Vite
│   └── src/
│       ├── pages/                 HostPage (+ webcam), GuestPage (+ video canvas), HomePage
│       ├── components/            LatencyDashboard, PipelineAnalysis
│       ├── hooks/                 useHostRoom (+ webcam), useGuestRoom (+ VideoPlayer), useTimings
│       ├── lib/                   AudioPipeline, RoomSocket, TtsPlayer, VideoPlayer
│       └── state/                 host/guest reducers + message handlers (+ video events)
├── brivva-frames/                 Rust frame extraction CLI (built, working)
├── docker-compose.yml             4 services
├── docker-compose.gpu.yml         GPU override (nvidia runtime for nllb + musetalk)
├── .env                           API keys
└── CLAUDE.md                      ← you are here
```

## Risks & Mitigations

- **MuseTalk dependency hell:** mmcv/mmpose/mmdet have strict version requirements. Fixed by using CUDA 11.8 + PyTorch 2.0.1 with prebuilt mmcv wheels. Docker isolates from host.
- **GPU memory contention:** NLLB + MuseTalk sharing GPU. Different CUDA runtimes in separate containers. May need to sequence, not parallelize.
- **Latency increase:** MuseTalk adds ~1-2s for a typical utterance (25-50 frames). Audio plays immediately, video arrives after. Acceptable for demo.
- **Video frame bandwidth:** 256x256 JPEG ~10-20KB × 25fps = ~250-500KB/s per language group via JSON base64. Fine for demo, optimize later with binary framing.

## Deadline

- **Thursday 6pm** — stop working on lip-sync regardless of state
- **If working:** demo with video + audio Friday
- **If not working:** demo with audio only Friday (still impressive), mention lip-sync as scoped and in progress

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
