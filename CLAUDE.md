# Brivva Real-Time Translation Prototype

## Purpose

Demo prototype for Brivva technical interview. Real-time voice translation + lip-sync for live commerce.
I'm the top candidate out of 15. In-person meeting Friday Mar 21, 12PM at Yeongdeungpo Times Square coffee shop with Simon and CTO.

## Current Status (Mar 18 2026)

**v4 — Lip-sync pipeline working end-to-end.** Dual lip-sync backends (Wav2Lip + MuseTalk), switchable via env var.

- **Frontend:** https://brivva.pages.dev (Cloudflare Pages)
- **Backend:** Rust axum server (server-rs :3000) via cloudflared tunnel
- **Tunnel:** brivva-server.milliytechnology.org → localhost:3000
- **Repo:** https://github.com/TyposBro/brivva (private)

### AWS Deployment (Live)

- **GPU Instance:** g5.xlarge (i-0c0b95e319c20d355) — A10G 24GB, 4 vCPU, 16GB RAM
- **IP:** 15.165.39.99 (ap-northeast-2)
- **AMI:** Ubuntu 24.04 (ami-084a56dceed3eb9bb), 100GB gp3
- **SSH:** `brivva` (alias) or `ssh -i ~/.ssh/brivva-key.pem ubuntu@15.165.39.99`
- **Tunnel:** cloudflared `brivva-aws` (29f844ea-0954-4c32-8b21-30e1cf2ab580) → localhost:3000
- **URL:** https://brivva-server.milliytechnology.org → g5.xlarge via cloudflared
- **Running:** server-rs + stt-wrapper + nllb (CUDA) + wav2lip (CUDA + GFPGAN)
- **CPU Instance:** t3.small (i-08fbb51994a0c4217) — STOPPED, was temporary
- **Security Group:** sg-0431248a86ea5644c (ports 22, 3000)
- **Key Pair:** brivva-key (PEM at ~/.ssh/brivva-key.pem)
- **IAM Users:** typosbro (personal), azizbek (work) — both AdministratorAccess
- **AWS Account:** 132593557399
- **Cost:** g5.xlarge ~$1.006/hr — STOP when not in use

#### AWS CLI Profiles
```bash
aws <command> --profile azizbek   # work
aws <command> --profile typosbro  # personal
```

#### Manage GPU Instance
```bash
aws ec2 start-instances --instance-ids i-0c0b95e319c20d355 --profile azizbek
aws ec2 stop-instances --instance-ids i-0c0b95e319c20d355 --profile azizbek
```

#### SSH & Logs
```bash
brivva                                # SSH into instance
brivva 'docker ps'                    # Check containers
brivva 'cd ~/brivva && docker compose -f docker-compose.yml -f docker-compose.gpu.yml --profile wav2lip logs -f --tail 50'
```

#### Wav2Lip Enhancements (v5)
- **GFPGAN v1.4** face restoration after Wav2Lip inference (96x96 → sharp upscale)
- **Feathered blending** instead of hard rectangle paste (smooth edges)
- **Cached face detector** (was re-creating per request)
- **Frame interpolation** 25fps → 60fps (configurable via LIPSYNC_FPS env var)
- JPEG quality 90 (was 85)
- Host speaks (EN or KO) → guests pick EN/JA/ZH → each gets translated audio + lip-synced video
- STT: CF Nova-3 via stt-wrapper (streaming interims + finals)
- Translation: NLLB-200-distilled-600M (self-hosted, CPU — GPU reserved for lip-sync)
- TTS: ElevenLabs eleven_flash_v2_5 (API, 548-1440ms, 32 languages)
- Lip-sync: **Wav2Lip** (default, batched, faster) or **MuseTalk v1.5** (higher quality, slower)
- Dockerized: 5 containers (server-rs, stt-wrapper, nllb, wav2lip, musetalk)

### What's Done (v4)

- [x] Unified lip-sync API: both Wav2Lip and MuseTalk expose identical `POST /lipsync` endpoint
- [x] Feature flag: `LIPSYNC_HOST=wav2lip|musetalk` env var switches backend (default: wav2lip)
- [x] Wav2Lip container: CUDA 11.8 + PyTorch 2.0.1 + wav2lip_gan.pth, batched inference (16 frames/batch)
- [x] MuseTalk container: CUDA 11.8 + PyTorch 2.0.1 + mmcv + full MMLab stack
- [x] Full pipeline: STT → NLLB → TTS (buffered) → Lip-sync → audio+video sent together (synced)
- [x] Host streams webcam at 30fps, server stores latest_face (not forwarded to guests)
- [x] Guests only receive lip-synced video + translated audio (no raw face stream)
- [x] Fallback: if lip-sync fails, audio-only sent to guest
- [x] Frontend deployed to CF Pages with tunnel URL

### Performance (Measured on RTX 4060)

| Backend  | 3.7s audio (92 frames) | Per frame | Real-time? |
| -------- | ---------------------- | --------- | ---------- |
| Wav2Lip  | TBD (batched, ~16/batch)| TBD       | Expected yes |
| MuseTalk | 17.3s                  | ~188ms    | No (4.7x too slow) |

MuseTalk root causes: per-frame UNet without batching, mmpose face detection per call (~1s), no TensorRT.

### What Needs Testing

- Wav2Lip end-to-end (just built, not tested yet)
- Guest video playback sync with audio
- VideoPlayer queue rendering of lip-synced frames on canvas

## Architecture (v4)

```
Host Browser (/host)
  → Webcam streams face frames (256x256 JPEG, 30fps) → face:frame → server-rs
  → server-rs stores latest_face in Room (NOT forwarded to guests)
  → PCM linear16 @ 16kHz → WebSocket → server-rs (Rust/axum :3000)
  → stt-wrapper (:8766) → CF Nova-3 (streaming transcription)
  → NLLB (:8000) — per active language, parallel tokio::spawn
  → ElevenLabs TTS (API) — MP3 buffered (not streamed to guests)
  → Buffered MP3 + latest face → Lip-sync (:8100) /lipsync → JPEG frames
  → Audio (tts_start → MP3 blob → tts_end) + Video (video_start → frames → video_end)
  → Guest Browser: TtsPlayer plays audio, VideoPlayer renders frames on canvas

Host sees: mirrored webcam preview (local <video> element)
Guests see: lip-synced translated video + audio (synced delivery)
```

| Service     | Port | Tech                             | Latency            | GPU  | CUDA |
| ----------- | ---- | -------------------------------- | ------------------ | ---- | ---- |
| server-rs   | 3000 | Rust/axum, DashMap, mpsc         | orchestration      | No   | —    |
| stt-wrapper | 8766 | Python asyncio, CF Nova-3 API    | streaming          | No   | —    |
| NLLB        | 8000 | nllb-200-distilled-600M, FastAPI | ~1.5-2.4s CPU      | No   | —    |
| ElevenLabs  | API  | eleven_flash_v2_5                | 548-1440ms         | No   | —    |
| Wav2Lip     | 8100 | Wav2Lip+GAN, FastAPI             | batched, TBD       | Yes  | 11.8 |
| MuseTalk    | 8101 | MuseTalk v1.5, FastAPI           | ~188ms/frame       | Yes  | 11.8 |

## Lip-Sync Integration

### Unified API (wav2lip/server.py & musetalk/server.py)

Both backends expose identical endpoints:

```
POST /lipsync
  Body: { "audio_base64": "<base64 MP3>", "face_image_base64": "<base64 JPEG>" }
  Response: { "frames_base64": ["<JPEG>", ...], "fps": 25, "lipsync_ms": N }

GET /health
  Response: { "status": "healthy", "model": "wav2lip_gan|musetalk_v1.5", "device": "cuda" }
```

### Switching Backends

```bash
# Default: Wav2Lip (faster, batched inference)
docker compose -f docker-compose.yml -f docker-compose.gpu.yml up

# MuseTalk (higher quality, slower)
LIPSYNC_HOST=musetalk docker compose -f docker-compose.yml -f docker-compose.gpu.yml --profile musetalk up
```

Server-rs reads `LIPSYNC_HOST` env var → constructs `http://{LIPSYNC_HOST}:8100/lipsync` URL.

### Wav2Lip Container

- **Base:** nvidia/cuda:11.8.0-runtime-ubuntu22.04 (lighter than devel)
- **PyTorch:** 2.0.1 + cu118
- **Model:** wav2lip_gan.pth (~436MB) from HuggingFace
- **Face detection:** s3fd (same as MuseTalk, baked in)
- **Key advantage:** batched inference (16 frames/batch) — much faster than per-frame
- **No MMLab deps** — simpler, faster build

### MuseTalk Container

- **Base:** nvidia/cuda:11.8.0-devel-ubuntu22.04
- **PyTorch:** 2.0.1 + cu118
- **MMLab:** mmcv 2.0.1 (prebuilt wheels) + mmdet 3.1.0 + mmpose 1.1.0
- **Models:** musetalkV15/unet.pth, sd-vae-ft-mse, whisper-tiny, dwpose, face-parse-bisent
- **Weights:** ~6GB total (cloned via git-xet + wget)
- **Slower but higher quality** lip-sync output

### Pipeline Flow (pipeline.rs)

```
Host streams face:frame at 30fps → server stores latest_face in Room

do_tts_and_broadcast():
  1. Buffer ALL TTS MP3 chunks (don't send to guests yet)
  2. After TTS complete, grab latest_face from Room
  3. POST { audio_base64, face_image_base64 } to LIPSYNC_HOST:8100/lipsync
  4. On success: send audio + video together (synced):
     → tts_start → MP3 blob → tts_end
     → video_start → video_frame[] → video_end
  5. On failure: fallback to audio-only (tts_start → MP3 → tts_end)
```

### Host Face Streaming

1. HostPage starts webcam on mount (512x512, getUserMedia)
2. Mirrored preview via `<video>` element (`transform: scaleX(-1)`)
3. When recording starts, captures frames at 30fps (33ms interval)
4. Each frame: center-crop → 256x256 → JPEG base64 → `{ type: "face:frame", data }` via WS
5. handler.rs stores latest_face in Room (NOT forwarded to guests)

### Guest Playback

- Guests only receive synced audio + lip-synced video (no raw host face)
- `TtsPlayer` plays audio via Blob URL
- `VideoPlayer` renders lip-synced frames on `<canvas>` at target FPS
- Both arrive together after lip-sync processing completes

### Key Build Lessons

- **mmcv** must use prebuilt wheels (`-f https://download.openmmlab.com/mmcv/dist/cu118/torch2.0/index.html`)
- **numpy<2** required — xtcocotools compiled against numpy 1.x
- **huggingface_hub<0.24** required — diffusers 0.27.2 uses removed `cached_download`
- **s3fd weight** must be pre-downloaded at build time (runtime download gets corrupted by container restart)
- **MuseTalk expects relative paths** — WORKDIR must be `/app/MuseTalk`, symlink `models/` → `/app/models/`
- **get_landmark_and_bbox** expects file paths — bypass with direct numpy calls to mmpose/face_alignment

## WebSocket Protocol (v4)

Connection via query params:

- Host: `ws://host/api/room?role=host&sourceLang=en`
- Guest: `ws://host/api/room?role=guest&roomId=ABC123&lang=ja`

**Host → Server:**
- `[ArrayBuffer]` — PCM audio frames
- `{ "type": "face:frame", "data": "<base64>" }` — webcam frames (30fps while recording)
- `"host:end"` — close room

**Server → Host:**
- room:created, room:guest_count, interim, final, translation, tts_end, video_end

**Server → Guest (synced delivery — all sent after lip-sync completes):**
- room:joined, interim, final, translation
- tts_start → [MP3 binary blob] → tts_end (audio)
- video_start → video_frame[] (lip-synced JPEG) → video_end (video)
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

### Why CF Nova-3 (via stt-wrapper) over Whisper

- Streaming (interims while speaking) vs batch-only
- Better accuracy for short phrases (Whisper hallucinates on silence)
- No local GPU needed for STT

### Why NLLB over M2M100/LLM

- NLLB = Meta's successor to M2M100: better quality, 200 languages
- distilled-600M = half the size of M2M100-1.2B, faster inference
- Self-hosted = no network roundtrip, ~3x faster than LLM prompting

### Why ElevenLabs over Kokoro

- 32 languages with natural expressive voices
- API-based: no local GPU needed
- Tradeoff: API dependency + per-character cost

### Why dual lip-sync backends

- **Wav2Lip** (default): faster (batched inference), simpler deps, but lower visual quality ("uncanny" frozen face)
- **MuseTalk**: higher quality (GAN + sync loss), but ~5x too slow on RTX 4060 without optimization
- Unified API lets us switch instantly via env var, benchmark both, pick best for demo

### Fan-out optimization

- Translate ONCE per language group, broadcast to all N guests
- 50 JA guests = 1 NLLB + 1 TTS + 1 lip-sync call, not 50

## Bug Fixes Log

- **TTS queue freeze** — audio.play() Promise rejection left playing flag stuck. Fix: .catch() calls advance()
- **AudioContext unlock** — Browser autoplay policy. Fix: guest language picker calls AudioContext.resume() on click
- **URL.revokeObjectURL** — Must revoke blob URL after playback to prevent memory leaks
- **Whisper hallucination** — Random text on silence. Fix: switched to CF Nova-3
- **STT retry loop** — stt-wrapper may still be starting. Retries 10x, 3s apart
- **PCM encoding** — Float32→Int16: Math.max(-32768, Math.min(32767, float32 * 32768))
- **mmcv build failure** — No prebuilt wheel for CUDA 12.4. Fix: CUDA 11.8 + prebuilt wheel index URL
- **MuseTalk get_landmark_and_bbox** — Expects file paths. Fix: call mmpose/face_alignment directly with numpy
- **MuseTalk VAE** — get_latents_for_unet expects BGR numpy, not tensor. Fix: pass raw crop
- **s3fd corrupted download** — Container restart corrupts partial download. Fix: pre-download at build time
- **Webcam ref race** — Video element renders conditionally but stream set before mount. Fix: callback ref + always-rendered hidden video
- **MuseTalk CUDA OOM** — NLLB + MuseTalk both on GPU exhausted 7.62 GiB VRAM. Fix: moved NLLB to CPU (removed from docker-compose.gpu.yml), freeing full GPU for lip-sync
- **MuseTalk brown blob / stale face** — Two bugs: (1) `get_latents_for_unet` already creates [masked, ref] 8-channel latent, but code re-masked spatial lower half → corrupted UNet input. Fix: removed redundant masking. (2) Raw rectangle paste instead of face-parsing blend. Fix: use `get_image()` from musetalk.utils.blending with FaceParsing mask for seamless compositing

## Measured Latency

| Phase                              | Measured        | Notes                              |
| ---------------------------------- | --------------- | ---------------------------------- |
| STT (Nova-3 via stt-wrapper)       | streaming       | Interims arrive while speaking     |
| Translation (NLLB CPU)             | 1537-2434ms     | CPU only — GPU reserved for lip-sync |
| TTS (ElevenLabs, buffered)         | 711ms           | Buffered, not streamed to guests   |
| Lip-sync Wav2Lip                   | TBD             | Batched (16/batch), expected fast  |
| Lip-sync MuseTalk                  | 17.3s (92 fr)   | ~188ms/frame on RTX 4060           |
| **Total (audio only)**             | **~2.5-3.5s**   | STT + NLLB + TTS                   |
| **Total (Wav2Lip)**                | **TBD**         | Expected ~3-5s                     |
| **Total (MuseTalk)**               | **~20s**        | Too slow for real-time             |
| **Target**                         | **<300ms**      | Gap: ~10x (audio), ~65x (MuseTalk) |

## Rust Server Key Concepts

- **Room:** host_tx, guests DashMap, source_lang, latest_face (Option<String>)
- **Pipeline flow:** host audio → STT → final → spawn per-lang → translate → TTS (buffer) → lip-sync → send audio+video together
- **LIPSYNC_URL:** reads `LIPSYNC_HOST` env var, defaults to `wav2lip`, constructs `http://{host}:8100/lipsync`

## Frontend Architecture

- `src/lib/AudioPipeline.ts` — mic capture, Float32→Int16 conversion
- `src/lib/RoomSocket.ts` — WebSocket connect, sendAudio, sendJson
- `src/lib/TtsPlayer.ts` — Blob URL audio playback queue
- `src/lib/VideoPlayer.ts` — Canvas JPEG frame renderer (queue-based for lip-sync)
- `src/hooks/useHostRoom.ts` — orchestrator: WebSocket + AudioPipeline + webcam 30fps streaming
- `src/hooks/useGuestRoom.ts` — guest: TtsPlayer + VideoPlayer + RoomSocket
- `src/state/host/` — reducer + message handler
- `src/state/guest/` — reducer + message handler (tts_start/end, video_start/frame/end)
- `src/pages/HostPage.tsx` — webcam preview (mirrored), room code, audio recorder
- `src/pages/GuestPage.tsx` — lip-synced video canvas, language picker, translations

## Docker Services

```yaml
# docker-compose.yml — 5 containers
server-rs: Rust axum, port 3000, LIPSYNC_HOST=${LIPSYNC_HOST:-wav2lip}
stt-wrapper: Python asyncio, port 8766
nllb: FastAPI + nllb-200-distilled-600M, port 8000, CPU (GPU reserved for lip-sync)
wav2lip: FastAPI + wav2lip_gan, port 8100, GPU (default lip-sync)
musetalk: FastAPI + MuseTalk v1.5, port 8101, GPU (profile: musetalk, opt-in)

# Default (Wav2Lip):
docker compose -f docker-compose.yml -f docker-compose.gpu.yml up

# MuseTalk:
LIPSYNC_HOST=musetalk docker compose -f docker-compose.yml -f docker-compose.gpu.yml --profile musetalk up
```

## Project Structure

```
brivva/
├── server-rs/                     Rust axum WebSocket server
│   ├── Dockerfile                 Multi-stage: rust:1.85 builder + debian slim runtime
│   ├── Cargo.toml                 axum, dashmap, reqwest, tokio, serde, base64
│   └── src/
│       ├── main.rs                Entry, router
│       ├── types.rs               Lang, Room (latest_face), Guest, ServerMsg
│       ├── pipeline.rs            STT → NLLB → TTS → Lip-sync (LIPSYNC_URL)
│       └── room/handler.rs        WS host/guest handlers, face:frame storage
├── stt-wrapper/                   STT proxy (CF Nova-3)
│   └── server.py
├── nllb/                          NLLB translation server
│   ├── Dockerfile / Dockerfile.gpu
│   └── server.py                  POST /translate
├── wav2lip/                       Wav2Lip lip-sync server (default)
│   ├── Dockerfile                 CUDA 11.8 runtime + PyTorch 2.0.1
│   ├── requirements.txt
│   ├── download_weights.sh        wav2lip_gan.pth + s3fd.pth
│   └── server.py                  POST /lipsync (batched inference, 16/batch)
├── musetalk/                      MuseTalk lip-sync server (opt-in)
│   ├── Dockerfile                 CUDA 11.8 devel + PyTorch 2.0.1 + mmcv
│   ├── requirements.txt
│   ├── download_weights.sh        git-xet clone + wget
│   └── server.py                  POST /lipsync (per-frame inference)
├── frontend/                      React 19 + TypeScript + Vite
│   └── src/
│       ├── pages/                 HostPage, GuestPage, HomePage
│       ├── hooks/                 useHostRoom (webcam 30fps), useGuestRoom
│       ├── lib/                   AudioPipeline, RoomSocket, TtsPlayer, VideoPlayer
│       └── state/                 host/guest reducers + message handlers
├── docker-compose.yml             5 services (wav2lip default, musetalk opt-in profile)
├── docker-compose.gpu.yml         GPU override (nvidia runtime)
├── .env                           API keys + LIPSYNC_HOST
└── CLAUDE.md                      ← you are here
```

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
