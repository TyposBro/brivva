# Brivva Real-Time Translation Prototype

## Current Status (Mar 20 2026)

**v8 — FFmpeg RTMP streaming wired.** Host webcam + translated TTS audio → FFmpeg → RTMP push to Twitch/YouTube/etc. 13-platform dashboard with zero-config UX.

- **Frontend:** https://brivva.pages.dev (Cloudflare Pages)
- **Backend:** Rust axum server (server-rs :3000) via cloudflared tunnel
- **Tunnel:** brivva-server.milliytechnology.org → localhost:3000
- **Repo:** https://github.com/TyposBro/brivva (private)

### What's Working

- Host speaks (KO/EN) → real-time STT → NLLB translation → ElevenLabs TTS → translated audio to guests
- FFmpeg RTMP streaming: host webcam (video) + TTS audio → H.264+AAC → FLV → RTMP push to platforms
- Session-room linking: sessionId query param connects REST sessions to WebSocket rooms
- YouTube OAuth2 flow → auto-create broadcasts per language via YouTube Data API v3
- Twitch RTMP streaming → tested and working (stream key + auto RTMP URL)
- Multi-platform RTMP: 13 platforms (YouTube auto, rest manual RTMP URL + stream key)
- Zero-config UX: magic paste, credential vault, key-only mode, deep links
- Dashboard: session creation with platform picker, voice management, past sessions
- Session page: stream cards + "Start Broadcasting" button → links to host page
- SQLite persistence: users, voices, sessions, streams, platform_credentials
- Voice cloning: ElevenLabs /v1/voices/add → persistent cloned voices (working)
- Dockerized: 3 containers (server-rs + ffmpeg, stt-wrapper, nllb)

### Platform Integration Status

| Platform                                    | Status                           | Notes                                                 |
| ------------------------------------------- | -------------------------------- | ----------------------------------------------------- |
| YouTube                                     | OAuth ready, 24hr review pending | Auto-creates broadcasts via API                       |
| Twitch                                      | Working                          | Stream key + known RTMP URL                           |
| Instagram                                   | Manual RTMP                      | Key-only mode (known base URL)                        |
| TikTok                                      | Manual RTMP                      | Requires 1000+ followers                              |
| Coupang Live                                | Needs partnership                | Korean business registration + seller API partnership |
| Naver Shopping Live                         | Needs partnership                | Korean business registration + partner agreement      |
| Rakuten Live                                | Needs partnership                | Japanese business registration                        |
| Douyin/Taobao/Kuaishou/Xiaohongshu/Bilibili | Needs partnership                | Chinese business registration + platform APIs         |
| Custom RTMP                                 | Working                          | Any RTMP/RTMPS endpoint                               |

### Fundamental Architecture: 1 Stream = 1 Account = 1 Language

**RTMP platforms (Twitch, Instagram, etc.) only allow ONE ingest stream per account.** You cannot push 3 languages to the same Twitch stream key — the 2nd and 3rd connections will be rejected.

This means multi-language distribution requires:

- **1 platform account per language per platform** (e.g., twitch.tv/brivva_en, twitch.tv/brivva_ja, twitch.tv/brivva_zh)
- **YouTube is the exception** — OAuth API can auto-create multiple broadcasts on one account
- The dashboard must model this as **per-language stream destinations**, not "enable Twitch for all languages"

**Current focus:** Get 1 stream → 1 account → 1 language working correctly end-to-end. Then expand to per-language assignment UI.

**Phase 1 (now):** Dashboard creates 1 stream per session (1 platform, 1 language, 1 stream key). Prove FFmpeg RTMP works.
**Phase 2:** Dashboard allows assigning different platforms/accounts per language.
**Phase 3:** YouTube auto-creates N broadcasts per language via API on a single account.

### What's NOT Working / TODO

- [ ] **1:1 stream model:** Dashboard currently duplicates the same stream key across all languages (broken). Need to create exactly 1 FFmpeg stream per unique RTMP endpoint.
- [ ] **Audio-video sync timestamps:** TTS audio arrives in bursts (every 3-5s) but video is continuous 30fps. Each utterance needs a timestamp so FFmpeg knows exactly where to place translated audio in the video timeline.
- [ ] YouTube broadcast transition to "live" (24hr review pending)
- [ ] Emotion-conditioned TTS style_params not reaching ElevenLabs
- [ ] **Per-language platform assignment UI** (Phase 2)

### Production Hardening (Post-Demo Priority)

- [ ] **Timestamp-based audio sync:** Each TTS utterance needs `utterance_start`/`utterance_end` timestamps relative to the video stream. FFmpeg must place audio at the correct position, not just append.
- [ ] **Session cleanup:** DashMap rooms + FFmpeg processes killed on WebSocket close
- [ ] **Network resilience:** Buffer/reconnect on WS drops, handle API 503s with fallback
- [ ] **Sync drift:** After 45+ min, translated audio drifts — needs correction
- [ ] **Observability:** Grafana/Prometheus metrics for API latency, pipeline errors
- [ ] **Self-healing:** Auto-restart pipeline on crash, resume live streams
- [ ] **Global relay:** CDN for non-Korea viewers (~400ms latency currently)
- [ ] **Stress testing:** Concurrent sessions, FFmpeg process limits

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

## Architecture (v8)

```
Host Browser (/host?sessionId=xxx)
  → PCM linear16 @ 16kHz → WebSocket → server-rs (Rust/axum :3000)
  → Webcam 30fps JPEG → face:frame → server-rs stores latest_face
  → stt-wrapper (:8766) → Deepgram Nova-3 (streaming transcription)
  → NLLB (:8000) — per active language, parallel tokio::spawn
  → ElevenLabs TTS (API) — MP3 buffered
  → MP3 → Guest Browser: TtsPlayer plays translated audio
  → MP3 → decode to PCM → FFmpeg audio FIFO (per-language stream)
  → Webcam frames → FFmpeg video stdin (all streams)
  → FFmpeg per stream: H.264 + AAC → FLV → RTMP push to platform

Dashboard (/dashboard)
  → YouTube OAuth → connect account
  → Create session → select platforms + languages
  → YouTube: auto-create broadcasts via API
  → Others: user pastes RTMP URL + stream key (magic paste / vault)
  → Session page → "Start Broadcasting" → HostPage with sessionId
  → Host WS connect reads session streams from DB → starts FFmpeg per stream
```

| Service     | Port | Tech                         | Latency       | GPU |
| ----------- | ---- | ---------------------------- | ------------- | --- |
| server-rs   | 3000 | Rust/axum, DashMap, sqlx     | orchestration | No  |
| stt-wrapper | 8766 | Python asyncio, Deepgram API | streaming     | No  |
| NLLB        | 8000 | nllb-200-distilled-600M      | ~1.5-2.4s     | No  |
| ElevenLabs  | API  | eleven_flash_v2_5            | 548-1440ms    | No  |

## Multi-Platform Streaming

### Supported Platforms (13)

| Platform               | Region | Auto      | Default RTMP                                |
| ---------------------- | ------ | --------- | ------------------------------------------- |
| YouTube                | Global | Yes (API) | Auto-created                                |
| Instagram              | Global | No        | rtmps://live-upload.instagram.com:443/rtmp/ |
| TikTok                 | Global | No        | Dynamic (user pastes)                       |
| Twitch                 | Global | No        | rtmp://live.twitch.tv/app/                  |
| Coupang Live           | Korea  | No        | Dynamic                                     |
| Naver Shopping Live    | Korea  | No        | Dynamic                                     |
| Rakuten Live           | Japan  | No        | Dynamic                                     |
| Douyin (抖音)          | China  | No        | Dynamic                                     |
| Taobao Live (淘宝直播) | China  | No        | Dynamic                                     |
| Kuaishou (快手)        | China  | No        | rtmp://live.kuaishou.com/live/              |
| Xiaohongshu (小红书)   | China  | No        | Dynamic                                     |
| Bilibili (哔哩哔哩)    | China  | No        | rtmp://live-push.bilivideo.com/live-bvc/    |
| Custom RTMP            | Other  | No        | User-provided                               |

### How It Works

- **YouTube**: OAuth2 → auto-create broadcast + stream per language → bind → get RTMP key
- **All others**: User copies stream key from platform, pastes into Brivva
- **Zero-config UX features:**
  - Magic Paste: paste any RTMP URL → auto-detect platform + split into URL/key
  - Key-Only Mode: Instagram, Twitch, Kuaishou, Bilibili hide RTMP URL (known base URLs)
  - Credential Vault: saved per-user in `platform_credentials` table, auto-fills next session
  - Deep Links: "Open Settings →" buttons link directly to each platform's streaming config
  - Region Grouping: Global, Korea, Japan, China, Other — with step-by-step help text

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

### Platform Credentials (Vault)

- `GET /api/credentials?user_id=...` → list saved platform credentials
- `POST /api/credentials` → upsert credential (auto-saved on session creation too)
- `DELETE /api/credentials?user_id=&platform=` → remove saved credential

## WebSocket Protocol

Connection via query params:

- Host: `ws://host/api/room?role=host&sourceLang=en`
- Guest: `ws://host/api/room?role=guest&roomId=ABC123&lang=ja`

**Host → Server:**

- `[ArrayBuffer]` — PCM audio frames
- `{ "type": "face:frame", "data": "<base64 JPEG>" }` — webcam frames (30fps while recording)
- `"host:end"` — close room

**Server → Host:**

- room:created, room:guest_count, interim, final, translation, tts_end

**Server → Guest:**

- room:joined, interim, final, translation
- tts_start → [MP3 binary blob] → tts_end (audio)
- room:closed

**FFmpeg RTMP (server-side, per stream):**

- Host face:frame → decode JPEG → push to all FFmpeg video stdin
- TTS MP3 → decode to PCM s16le → push to language-matched FFmpeg audio FIFO
- Audio FIFO writer pads silence (20ms zero chunks) between utterances

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

CREATE TABLE platform_credentials (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  platform TEXT NOT NULL,
  rtmp_url TEXT,
  stream_key TEXT,
  display_name TEXT,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  UNIQUE(user_id, platform)
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

| Phase                      | Measured      | Notes                   |
| -------------------------- | ------------- | ----------------------- |
| STT (Deepgram Nova-3)      | streaming     | Interims while speaking |
| Translation (NLLB)         | 1537-2434ms   | CPU inference           |
| TTS (ElevenLabs, buffered) | 711ms         | API call                |
| **Total (audio only)**     | **~2.5-3.5s** | STT + NLLB + TTS        |
| **Target**                 | **<300ms**    | Gap: ~10x               |

## Rust Server Key Concepts

- **AppState:** `rooms: Arc<DashMap<String, Room>>` + `db: SqlitePool`
- **Room:** host_tx, guests DashMap, source_lang, latest_face, session_id, rtmp_manager
- **Pipeline flow:** host audio → STT → final → spawn per-lang → translate → TTS → broadcast audio + push PCM to FFmpeg
- **FFmpeg RTMP:** RtmpManager spawns one FFmpeg per stream. Video via stdin (image2pipe JPEG 30fps), audio via named FIFO (s16le 44100Hz). Silence padding fills gaps between TTS utterances.
- **Session-Room link:** Host connects with `?sessionId=xxx` → handler reads streams from DB → starts FFmpeg per stream → updates session status to "live" with room_id
- **Routes:** REST API for OAuth, sessions, streams, voices, credentials (routes.rs)

## Frontend Architecture

- `src/lib/api.ts` — REST API client, PLATFORMS array (13 platforms), detectPlatform(), credential vault API
- `src/lib/AudioPipeline.ts` — mic capture, Float32→Int16 conversion
- `src/lib/RoomSocket.ts` — WebSocket connect, sendAudio, sendJson
- `src/lib/TtsPlayer.ts` — Blob URL audio playback queue
- `src/hooks/useHostRoom.ts` — orchestrator: WebSocket + AudioPipeline
- `src/hooks/useGuestRoom.ts` — guest: TtsPlayer + RoomSocket
- `src/state/host/` — reducer + message handler
- `src/state/guest/` — reducer + message handler
- `src/pages/DashboardPage.tsx` — YouTube connect, voice mgmt, magic paste, credential vault, key-only platforms
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
│       ├── routes.rs              REST API handlers (OAuth, sessions, voices, streams, credentials)
│       ├── platform.rs            PlatformProvider trait, detect_platform(), settings_url()
│       ├── pipeline.rs            STT → NLLB → TTS pipeline
│       ├── ffmpeg.rs              RtmpManager: FFmpeg spawn, video stdin, audio FIFO, MP3→PCM decode
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
- **PCM encoding** — Float32→Int16: Math.max(-32768, Math.min(32767, float32 \* 32768))
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
