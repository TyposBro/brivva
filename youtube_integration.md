# YouTube Multi-Language Live Stream Integration

## Overview

Host signs up → connects YouTube account → clones voice (saved permanently) → creates session with title + target languages → Brivva creates N parallel YouTube live streams → host talks → each stream gets real-time translated audio with cloned voice.

## Architecture

```
Host Browser
  → Webcam (720p/1080p) + Mic audio → WebSocket → Brivva Server

Brivva Server:
  → Original audio → STT (Deepgram Nova-3) → transcript
  → Per target language:
      → NLLB translate → ElevenLabs TTS (cloned voice) → translated audio
      → FFmpeg: host video + translated audio → RTMP → YouTube

YouTube Live Streams:
  Stream 1 (Korean/source):  host video + original audio    → rtmp://youtube/key1
  Stream 2 (English):        host video + EN dubbed audio   → rtmp://youtube/key2
  Stream 3 (Japanese):       host video + JA dubbed audio   → rtmp://youtube/key3
  Stream 4 (Chinese):        host video + ZH dubbed audio   → rtmp://youtube/key4
```

## Implementation Progress

### Phase 1: YouTube OAuth + Broadcast Creation — DONE (backend)

**1.1 Google Cloud Setup** — DONE
- [x] GCP project `brivva-live` created, YouTube Data API v3 enabled
- [ ] OAuth2 web credentials (must create in GCP Console UI — not available via gcloud CLI)

**1.2 Backend: OAuth2 Flow** — DONE (`youtube.rs` + `routes.rs`)
- [x] `GET /auth/youtube?user_id=...` → redirect to Google OAuth consent screen
- [x] `GET /auth/youtube/callback?code=&state=user_id` → exchange code, store tokens, redirect to dashboard
- [x] Token refresh with 60s buffer (`ensure_valid_token`)
- [x] Channel info fetched on OAuth complete

**1.3 Backend: Broadcast Management** — DONE (`routes.rs`)
- [x] `POST /api/sessions` → create session + N YouTube live broadcasts per language
  - liveBroadcasts.insert per language (translated titles: "Title [EN]")
  - liveStreams.insert per language (RTMP ingest)
  - liveBroadcasts.bind stream to broadcast
  - Store broadcast IDs + stream keys in DB
- [x] `GET /api/sessions?user_id=...` → list sessions
- [x] `GET /api/sessions/:id` → session + streams detail
- [x] `DELETE /api/sessions/:id` → transition all broadcasts to "complete"

**1.4 Backend: Voice Management** — DONE (`routes.rs`)
- [x] `POST /api/voices` → clone via ElevenLabs, save to DB (persistent)
- [x] `GET /api/voices?user_id=...` → list saved voices
- [x] `DELETE /api/voices/:id` → delete from DB + ElevenLabs

**1.5 Backend: Database** — DONE (`db.rs`)
- [x] SQLite via sqlx: users, voices, sessions, streams tables
- [x] Full CRUD for all entities
- [x] Docker volume for persistence

**1.6 Frontend: Dashboard + Session UI** — DONE
- [x] `api.ts` — REST API client (user, sessions, voices, YouTube auth)
- [x] `DashboardPage.tsx` — YouTube connect, voice selection, session creation form
- [x] `SessionPage.tsx` — live session with stream cards (RTMP URLs, broadcast IDs, status)
- [x] `App.tsx` — /dashboard, /session/:id routes added
- [x] `HomePage.tsx` — "Stream Dashboard" button added
- [x] `App.css` — full dashboard + session page styles

### Phase 2: FFmpeg RTMP Streaming

**2.1 Video Pipeline**
- Host sends webcam frames via WebSocket (already implemented)
- Server accumulates frames into raw video pipe for FFmpeg
- Alternative: host sends MediaRecorder chunks (WebM/H264) → more efficient

**2.2 Per-Language FFmpeg Process**
```bash
# For each target language:
ffmpeg \
  -f rawvideo -pix_fmt yuv420p -s 1280x720 -r 30 -i pipe:0 \
  -f mp3 -i pipe:1 \
  -c:v libx264 -preset ultrafast -tune zerolatency -b:v 2500k \
  -c:a aac -b:a 128k -ar 44100 \
  -f flv "rtmp://a.rtmp.youtube.com/live2/{STREAM_KEY}"
```

**2.3 Original Language Stream**
- Forward host video + original audio directly (no TTS)
- Uses same FFmpeg pattern but with original audio pipe

**2.4 Audio Synchronization**
- TTS audio arrives in chunks per utterance (~2-4s behind real-time)
- Need silence padding between utterances to keep audio timeline aligned
- FFmpeg `-async 1` or custom audio timeline management

### Phase 3: Persistent Data (SQLite)

**3.1 Schema**
```sql
CREATE TABLE users (
  id TEXT PRIMARY KEY,        -- UUID
  youtube_channel_id TEXT,
  youtube_channel_name TEXT,
  youtube_access_token TEXT,
  youtube_refresh_token TEXT,
  youtube_token_expires_at INTEGER,
  created_at INTEGER
);

CREATE TABLE voices (
  id TEXT PRIMARY KEY,        -- UUID
  user_id TEXT REFERENCES users(id),
  elevenlabs_voice_id TEXT,   -- ElevenLabs voice ID (don't delete)
  name TEXT,                  -- "My voice", "Host 2", etc.
  created_at INTEGER
);

CREATE TABLE sessions (
  id TEXT PRIMARY KEY,        -- UUID
  user_id TEXT REFERENCES users(id),
  voice_id TEXT REFERENCES voices(id),
  title TEXT,
  source_lang TEXT,
  target_langs TEXT,          -- JSON array: ["en","ja","zh"]
  status TEXT,                -- "setup", "live", "ended"
  room_id TEXT,               -- Brivva room code
  created_at INTEGER
);

CREATE TABLE streams (
  id TEXT PRIMARY KEY,
  session_id TEXT REFERENCES sessions(id),
  lang TEXT,
  youtube_broadcast_id TEXT,
  youtube_stream_id TEXT,
  youtube_stream_key TEXT,
  rtmp_url TEXT,
  status TEXT,                -- "created", "live", "ended"
  created_at INTEGER
);
```

**3.2 Voice Persistence**
- After voice clone: store voice_id in `voices` table
- Do NOT delete from ElevenLabs on room close
- Host can manage voices: list, delete, re-record
- Session creation picks from saved voices

### Phase 4: Multi-Platform (Future)

All major live commerce platforms use RTMP:
- **YouTube**: `rtmp://a.rtmp.youtube.com/live2/{key}`
- **Twitch**: `rtmp://live.twitch.tv/app/{key}`
- **TikTok**: `rtmp://push.tiktok.com/live/{key}`
- **Instagram**: RTMP via Instagram Live Producer
- **Coupang Live**: RTMP (enterprise API)
- **Rakuten**: RTMP (via Rakuten Live Commerce API)
- **Naver Shopping Live**: RTMP

Same FFmpeg pipeline, just different RTMP endpoints per platform.

## YouTube API Details

### Quota Budget (default 10,000 units/day)
| Operation | Cost | Per Session (4 langs) |
|-----------|------|-----------------------|
| liveBroadcasts.insert | 50 | 200 |
| liveStreams.insert | 50 | 200 |
| liveBroadcasts.bind | 50 | 200 |
| liveBroadcasts.transition | 50 | 200 |
| **Total per session** | | **800** |

12 sessions/day within default quota. Request increase for production.

### Required Scopes
- `https://www.googleapis.com/auth/youtube.force-ssl` — manage broadcasts
- `https://www.googleapis.com/auth/youtube.readonly` — read channel info

### Broadcast Lifecycle
1. `insert` → status: "complete" (metadata ready)
2. `bind` stream to broadcast
3. Start pushing RTMP (FFmpeg)
4. `transition` → "testing" (preview)
5. `transition` → "live" (public)
6. `transition` → "complete" (end stream)

## Latency Analysis

| Phase | Latency | Notes |
|-------|---------|-------|
| Host → Server (WebSocket) | ~50ms | Cloudflare tunnel |
| STT (Deepgram Nova-3) | ~500ms | Streaming, includes endpointing |
| Translation (NLLB) | ~1-2s | CPU inference |
| TTS (ElevenLabs) | ~500-1000ms | API call |
| FFmpeg encoding | ~100-500ms | ultrafast preset |
| RTMP push to YouTube | ~1-2s | Network |
| YouTube broadcast delay | ~10-30s | YouTube's own buffer |
| **Total (translation path)** | **~13-36s** | Acceptable for live commerce |
| **Original language path** | **~11-32s** | Just YouTube's delay |

The ~3-5s translation overhead is hidden within YouTube's own 10-30s broadcast delay.

## Environment Variables (New)

```env
GOOGLE_CLIENT_ID=...
GOOGLE_CLIENT_SECRET=...
GOOGLE_REDIRECT_URI=https://brivva.pages.dev/auth/youtube/callback
DATABASE_URL=sqlite:brivva.db
```

## Files Changed

```
server-rs/
  Cargo.toml          + sqlx, chrono, reqwest/form — DONE
  Dockerfile          + libsqlite3 (builder + runtime) — DONE
  src/
    main.rs           AppState (rooms + db), all REST routes registered — DONE
    db.rs             NEW: SQLite init + CRUD (users, voices, sessions, streams) — DONE
    youtube.rs        NEW: OAuth2 + broadcast/stream management — DONE
    routes.rs         NEW: REST API handlers — DONE
    room/handler.rs   Updated to use AppState — DONE
    ffmpeg.rs         Phase 2 (not started)

docker-compose.yml    + Google env vars, DATABASE_URL, db-data volume — DONE

frontend/
  src/
    lib/
      api.ts              NEW: REST API client — DONE
    pages/
      DashboardPage.tsx   NEW: YouTube connect, voice mgmt, session form — DONE
      SessionPage.tsx     NEW: stream cards, RTMP URLs, end session — DONE
      HomePage.tsx        + Dashboard button — DONE
    App.tsx               + /dashboard, /session/:id routes — DONE
    App.css               + dashboard + session styles — DONE
```
