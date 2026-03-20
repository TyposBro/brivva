# Brivva Real-Time Translation Prototype

## Current Status (Mar 20 2026)

**v6 — Multi-platform streaming dashboard.** YouTube OAuth + 13 platform RTMP streaming. Lip-sync removed (audio-only translation). Production-ready dashboard UI.

- **Frontend:** https://brivva.pages.dev (Cloudflare Pages)
- **Backend:** Rust axum server (server-rs :3000) via cloudflared tunnel
- **Tunnel:** brivva-server.milliytechnology.org → localhost:3000
- **Repo:** https://github.com/TyposBro/brivva (private)

### What's Working

- Host speaks (KO/EN) → real-time STT → NLLB translation → ElevenLabs TTS → translated audio to guests
- YouTube OAuth2 flow → auto-create broadcasts per language via YouTube Data API v3
- Multi-platform RTMP: 13 platforms (YouTube auto, rest manual RTMP URL + stream key)
- Dashboard: session creation with platform picker, voice management, past sessions
- Session page: stream cards with status, RTMP URLs, broadcast IDs
- SQLite persistence: users, voices, sessions, streams
- Voice cloning: ElevenLabs /v1/voices/add → persistent cloned voices
- Dockerized: 3 containers (server-rs, stt-wrapper, nllb)

### What's NOT Working / TODO

- [ ] FFmpeg RTMP muxing not wired into pipeline (code written in ffmpeg.rs, not connected)
- [ ] YouTube broadcast transition to "live" (requires RTMP push first)
- [ ] Emotion-conditioned TTS style_params not reaching ElevenLabs

### AWS Deployment (Live)

- **GPU Instance:** g5.xlarge (i-0c0b95e319c20d355) — A10G 24GB, 4 vCPU, 16GB RAM
- **IP:** 15.165.39.99 (ap-northeast-2)
- **AMI:** Ubuntu 24.04 (ami-084a56dceed3eb9bb), 100GB gp3
- **SSH:** `brivva` (alias) or `ssh -i ~/.ssh/brivva-key.pem ubuntu@15.165.39.99`
- **Tunnel:** cloudflared `brivva-aws` (29f844ea-0954-4c32-8b21-30e1cf2ab580) → localhost:3000
- **URL:** https://brivva-server.milliytechnology.org → g5.xlarge via cloudflared
- **Running:** server-rs + stt-wrapper + nllb
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
brivva 'cd ~/brivva && docker compose logs -f --tail 50'
```

### Lip-Sync (Removed)

Wav2Lip and MuseTalk containers were removed. Lip-sync was a known industry problem — single reference frame → N output frames creates uncanny frozen body. Decision: demo audio-only translation (works well), mention lip-sync as scoped R&D.

## Architecture (v6)

```
Host Browser (/host)
  → PCM linear16 @ 16kHz → WebSocket → server-rs (Rust/axum :3000)
  → stt-wrapper (:8766) → Deepgram Nova-3 (streaming transcription)
  → NLLB (:8000) — per active language, parallel tokio::spawn
  → ElevenLabs TTS (API) — MP3 streamed to guests
  → Guest Browser: TtsPlayer plays translated audio

Dashboard (/dashboard)
  → YouTube OAuth → connect account
  → Create session → select platforms + languages
  → YouTube: auto-create broadcasts via API
  → Others: user pastes RTMP URL + stream key
  → Session page shows stream cards with status

Future: FFmpeg muxes host video + translated audio → RTMP push to each platform
```

| Service     | Port | Tech                          | Latency       | GPU |
| ----------- | ---- | ----------------------------- | ------------- | --- |
| server-rs   | 3000 | Rust/axum, DashMap, sqlx      | orchestration | No  |
| stt-wrapper | 8766 | Python asyncio, Deepgram API  | streaming     | No  |
| NLLB        | 8000 | nllb-200-distilled-600M       | ~1.5-2.4s     | No  |
| ElevenLabs  | API  | eleven_flash_v2_5             | 548-1440ms    | No  |

## Multi-Platform Streaming

### Supported Platforms (13)

| Platform | Region | Auto | Default RTMP |
|----------|--------|------|-------------|
| YouTube | Global | Yes (API) | Auto-created |
| Instagram | Global | No | rtmps://live-upload.instagram.com:443/rtmp/ |
| TikTok | Global | No | Dynamic (user pastes) |
| Twitch | Global | No | rtmp://live.twitch.tv/app/ |
| Coupang Live | Korea | No | Dynamic |
| Naver Shopping Live | Korea | No | Dynamic |
| Rakuten Live | Japan | No | Dynamic |
| Douyin (抖音) | China | No | Dynamic |
| Taobao Live (淘宝直播) | China | No | Dynamic |
| Kuaishou (快手) | China | No | rtmp://live.kuaishou.com/live/ |
| Xiaohongshu (小红书) | China | No | Dynamic |
| Bilibili (哔哩哔哩) | China | No | rtmp://live-push.bilivideo.com/live-bvc/ |
| Custom RTMP | Other | No | User-provided |

### How It Works

- **YouTube**: OAuth2 → auto-create broadcast + stream per language → bind → get RTMP key
- **All others**: User copies RTMP URL + stream key from platform dashboard, pastes into Brivva
- **Dashboard UI**: Platforms grouped by region, help text guides users step-by-step
- **Pre-filled RTMP URLs**: Instagram, Twitch, Kuaishou, Bilibili have known base URLs

## REST API

### YouTube OAuth
- `GET /auth/youtube?user_id=...` → redirect to Google consent
- `GET /auth/youtube/callback?code=&state=` → exchange token, store in DB, redirect to dashboard

### User
- `GET /api/user?user_id=...` → user info + YouTube connection status

### Sessions
- `POST /api/sessions` → create session + streams (YouTube auto, others manual)
- `GET /api/sessions?user_id=...` → list sessions
- `GET /api/sessions/:id` → session detail + streams
- `DELETE /api/sessions/:id` → end session (transition YouTube broadcasts to complete)

### Streams
- `POST /api/sessions/:id/streams` → add stream to existing session
- `DELETE /api/sessions/:session_id/streams/:stream_id` → remove stream

### Voices
- `POST /api/voices` → clone via ElevenLabs, save to DB
- `GET /api/voices?user_id=...` → list saved voices
- `DELETE /api/voices/:id` → delete from DB + ElevenLabs

## WebSocket Protocol

Connection via query params:

- Host: `ws://host/api/room?role=host&sourceLang=en`
- Guest: `ws://host/api/room?role=guest&roomId=ABC123&lang=ja`

**Host → Server:**
- `[ArrayBuffer]` — PCM audio frames
- `"host:end"` — close room

**Server → Host:**
- room:created, room:guest_count, interim, final, translation, tts_end

**Server → Guest:**
- room:joined, interim, final, translation
- tts_start → [MP3 binary blob] → tts_end (audio)
- room:closed

## Database Schema (SQLite)

```sql
CREATE TABLE users (
  id TEXT PRIMARY KEY,
  youtube_channel_id TEXT,
  youtube_channel_name TEXT,
  youtube_access_token TEXT,
  youtube_refresh_token TEXT,
  youtube_token_expires_at INTEGER,
  created_at INTEGER NOT NULL
);

CREATE TABLE voices (
  id TEXT PRIMARY KEY,
  user_id TEXT REFERENCES users(id),
  elevenlabs_voice_id TEXT,
  name TEXT,
  created_at INTEGER
);

CREATE TABLE sessions (
  id TEXT PRIMARY KEY,
  user_id TEXT REFERENCES users(id),
  voice_id TEXT REFERENCES voices(id),
  title TEXT,
  source_lang TEXT,
  target_langs TEXT,          -- JSON array: ["en","ja","zh"]
  status TEXT,                -- "setup" | "live" | "ended"
  room_id TEXT,
  created_at INTEGER
);

CREATE TABLE streams (
  id TEXT PRIMARY KEY,
  session_id TEXT REFERENCES sessions(id),
  lang TEXT,
  platform TEXT,              -- "youtube", "instagram", "coupang", etc.
  platform_broadcast_id TEXT,
  platform_stream_id TEXT,
  stream_key TEXT,
  rtmp_url TEXT,
  status TEXT,
  created_at INTEGER
);
```

## Key Technical Decisions

### Why Rust axum over CF Workers (v2 → v3 migration)

- CF Workers AI M2M100 = 394-870ms + network. NLLB localhost = 82-164ms
- Durable Objects add complexity (DO pinning, isolate eviction, no GPU)
- All services co-located eliminates ~170-350ms network overhead per utterance
- Persistent WebSocket connections (Workers have 30s idle timeout issues)

### Why DashMap over HashMap+Mutex

- DashMap locks per-shard: concurrent room access without blocking all rooms
- HashMap+Mutex locks entire map: one slow room blocks all tokio tasks

### Why NLLB over M2M100/LLM

- NLLB = Meta's successor to M2M100: better quality, 200 languages
- distilled-600M = half the size of M2M100-1.2B, faster inference
- Self-hosted = no network roundtrip, ~3x faster than LLM prompting

### Fan-out optimization

- Translate ONCE per language group, broadcast to all N guests
- 50 JA guests = 1 NLLB + 1 TTS call, not 50

## Measured Latency

| Phase                        | Measured      | Notes                    |
| ---------------------------- | ------------- | ------------------------ |
| STT (Deepgram Nova-3)       | streaming     | Interims while speaking  |
| Translation (NLLB)          | 1537-2434ms   | CPU inference            |
| TTS (ElevenLabs, buffered)  | 711ms         | API call                 |
| **Total (audio only)**      | **~2.5-3.5s** | STT + NLLB + TTS        |
| **Target**                  | **<300ms**    | Gap: ~10x                |

## Rust Server Key Concepts

- **AppState:** `rooms: Arc<DashMap<String, Room>>` + `db: SqlitePool`
- **Room:** host_tx, guests DashMap, source_lang, frame_buffer
- **Pipeline flow:** host audio → STT → final → spawn per-lang → translate → TTS → broadcast audio
- **Routes:** REST API for OAuth, sessions, streams, voices (routes.rs)

## Frontend Architecture

- `src/lib/api.ts` — REST API client, PLATFORMS array (13 platforms with regions/help)
- `src/lib/AudioPipeline.ts` — mic capture, Float32→Int16 conversion
- `src/lib/RoomSocket.ts` — WebSocket connect, sendAudio, sendJson
- `src/lib/TtsPlayer.ts` — Blob URL audio playback queue
- `src/hooks/useHostRoom.ts` — orchestrator: WebSocket + AudioPipeline
- `src/hooks/useGuestRoom.ts` — guest: TtsPlayer + RoomSocket
- `src/state/host/` — reducer + message handler
- `src/state/guest/` — reducer + message handler
- `src/pages/DashboardPage.tsx` — YouTube connect, voice mgmt, multi-platform session creation
- `src/pages/SessionPage.tsx` — stream cards with platform badges, RTMP URLs, end session
- `src/pages/HostPage.tsx` — webcam preview, room code, audio recorder
- `src/pages/GuestPage.tsx` — language picker, translations, audio playback

## Project Structure

```
brivva/
├── server-rs/                     Rust axum WebSocket + REST server
│   ├── Dockerfile                 Multi-stage: rust:1.85 builder + debian slim runtime
│   ├── Cargo.toml                 axum, dashmap, reqwest, tokio, serde, sqlx, chrono
│   └── src/
│       ├── main.rs                Entry, AppState (rooms + db), router
│       ├── types.rs               Lang, Room (frame_buffer), Guest, ServerMsg
│       ├── db.rs                  SQLite init + CRUD (users, voices, sessions, streams)
│       ├── youtube.rs             OAuth2 + broadcast/stream management
│       ├── routes.rs              REST API handlers (OAuth, sessions, voices, streams)
│       ├── pipeline.rs            STT → NLLB → TTS pipeline
│       ├── ffmpeg.rs              RtmpManager (written, not yet wired)
│       └── room/handler.rs        WS host/guest handlers
├── stt-wrapper/                   STT proxy (Deepgram Nova-3)
│   └── server.py
├── nllb/                          NLLB translation server
│   ├── Dockerfile
│   └── server.py                  POST /translate
├── frontend/                      React 19 + TypeScript + Vite
│   └── src/
│       ├── pages/                 HomePage, DashboardPage, SessionPage, HostPage, GuestPage
│       ├── hooks/                 useHostRoom, useGuestRoom
│       ├── lib/                   api, AudioPipeline, RoomSocket, TtsPlayer
│       └── state/                 host/guest reducers + message handlers
├── docker-compose.yml             3 services (server-rs, stt-wrapper, nllb)
├── docker-compose.gpu.yml         GPU override (nvidia runtime)
├── .env                           API keys (Deepgram, ElevenLabs, Google OAuth)
└── CLAUDE.md                      ← you are here
```

## Docker Services

```yaml
# docker-compose.yml — 3 containers
server-rs: Rust axum, port 3000, REST + WebSocket
stt-wrapper: Python asyncio, port 8766, Deepgram Nova-3
nllb: FastAPI + nllb-200-distilled-600M, port 8000

docker compose up
```

## Environment Variables

```env
ELEVENLABS_API_KEY=...
DEEPGRAM_API_KEY=...
GOOGLE_CLIENT_ID=...
GOOGLE_CLIENT_SECRET=...
GOOGLE_REDIRECT_URI=https://brivva-server.milliytechnology.org/auth/youtube/callback
DATABASE_URL=sqlite:/data/brivva.db?mode=rwc
```

## Bug Fixes Log

- **TTS queue freeze** — audio.play() Promise rejection left playing flag stuck. Fix: .catch() calls advance()
- **AudioContext unlock** — Browser autoplay policy. Fix: guest language picker calls AudioContext.resume() on click
- **URL.revokeObjectURL** — Must revoke blob URL after playback to prevent memory leaks
- **Whisper hallucination** — Random text on silence. Fix: switched to Deepgram Nova-3
- **STT retry loop** — stt-wrapper may still be starting. Retries 10x, 3s apart
- **PCM encoding** — Float32→Int16: Math.max(-32768, Math.min(32767, float32 * 32768))
- **Redirect URI mismatch** — Must point to backend server, not frontend (Google OAuth requires exact match)
- **Old DB incompatible** — Adding platform column broke existing SQLite. Fix: delete DB file before restart

## About Brivva

- Real-time multilingual live commerce platform
- Voice translation for live streams
- Distribute to YouTube, TikTok, Naver, Instagram, Coupang, Rakuten, Chinese platforms simultaneously
- Founders have previous exit, seed round closing April
- 10 paying customers ($30-80K contracts)
- Stack: Rust backend, React/TS frontend, AWS
- Target: <300ms e2e latency
- Salary discussed: ₩80-100M + equity, remote OK (KST)
- Meeting: Friday Mar 21 12PM, Yeongdeungpo Times Square coffee shop
