# Brivva Real-Time Translation Prototype

## Purpose

Demo prototype for Brivva technical interview. Real-time voice translation for live commerce.
I'm the top candidate out of 15. Paid technical test coming (1hr with CTO).

## Current Status (Mar 15 2026)

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

## Architecture (v3)

```
Host Browser (/host)
  → PCM linear16 @ 16kHz → WebSocket → server-rs (Rust/axum :3000)
  → stt-wrapper (:8766) → CF Nova-3 (streaming transcription)
  → NLLB (:8000) — per active language, parallel tokio::spawn
  → ElevenLabs TTS (API) — streaming MP3
  → WebSocket → Guest Browser (Blob URL playback)
```

| Service     | Port | Tech                             | Latency       |
| ----------- | ---- | -------------------------------- | ------------- |
| server-rs   | 3000 | Rust/axum, DashMap, mpsc         | orchestration |
| stt-wrapper | 8766 | Python asyncio, CF Nova-3 API    | streaming     |
| NLLB        | 8000 | nllb-200-distilled-600M, FastAPI | 82-164ms GPU  |
| ElevenLabs  | API  | eleven_flash_v2_5                | 548-1440ms    |

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

### Fan-out optimization

- Translate ONCE per language group, broadcast to all N guests
- 50 JA guests = 1 NLLB call + 1 ElevenLabs call, not 50
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
| Overhead (routing)                 | ~11ms           | localhost, no network hops     |
| **Total from utterance finalized** | **~650-1600ms** |                                |
| **Target**                         | **<300ms**      | Gap: ~2-5x                     |

## Rust Server Key Concepts

- **AppState:** Arc<AppState> holds DashMap<String, Room> (rooms) + reqwest::Client (shared HTTP client)
- **Room:** host_tx (mpsc sender to host), guests DashMap<String, Guest>, source_lang
- **Guest:** tx (mpsc sender), lang, id
- **mpsc channels:** Each WebSocket connection gets a (tx, rx) pair. tx stored in Room, rx drives the WebSocket send loop
- **tokio::spawn:** Used for parallel per-language translation+TTS. One task per active language group
- **Pipeline flow:** handle_host_message → pipeline::process_utterance → spawn per-lang → translate → tts → broadcast

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
# docker-compose.yml — 3 containers
server-rs: Rust axum, port 3000, no GPU
stt-wrapper: Python asyncio, port 8766, no GPU (CF Nova-3 API proxy)
nllb: FastAPI + nllb-200-distilled-600M, port 8000, optional GPU
```

## Project Structure

```
brivva/
├── server-rs/                     Rust axum WebSocket server
│   ├── Dockerfile
│   └── src/
│       ├── main.rs                Entry, router, AppState
│       ├── types.rs               Lang, Room, Guest, ServerMsg
│       ├── pipeline.rs            STT → Translate → TTS pipeline
│       └── room/handler.rs        WebSocket host/guest handlers
├── stt-wrapper/                   STT proxy (CF Nova-3)
│   └── server.py                  asyncio WebSocket proxy
├── nllb/                          NLLB translation server
│   └── server.py                  FastAPI, POST /translate
├── frontend/                      React 19 + TypeScript + Vite
│   └── src/
│       ├── pages/                 HostPage, GuestPage, HomePage
│       ├── components/            LatencyDashboard, PipelineAnalysis
│       ├── hooks/                 useHostRoom, useGuestRoom, useTimings, hostReducer
│       ├── lib/                   AudioPipeline, RoomSocket, TtsPlayer
│       └── state/                 host/guest reducers + message handlers
├── docker-compose.yml             3 services
├── .env                           API keys
└── CLAUDE.md                      ← you are here
```

## About Brivva

- Real-time multilingual live commerce platform
- Voice translation + lip-sync for live streams
- Distribute to TikTok, Naver, Instagram, Rakuten simultaneously
- Founders have previous exit, seed round closing April
- 10 paying customers ($30-80K contracts)
- Stack: Rust backend, React/TS frontend, AWS
- Target: <300ms e2e latency
- Salary discussed: ₩80-100M, remote OK (KST), 3-month Rust ramp-up acknowledged
