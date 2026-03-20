# Multi-Platform Live Stream Integration

## Overview

Host signs up → connects YouTube account (optional) → selects voice → creates session with title + target languages + platforms → Brivva creates streams (YouTube auto via API, others use manual RTMP) → host talks → each stream gets real-time translated audio.

## Architecture

```
Host Browser (/dashboard)
  → Connect YouTube (OAuth2)
  → Create session: title, source lang, target langs, platforms
  → YouTube: auto-create broadcast + stream per language via API
  → Others: user pastes RTMP URL + stream key from platform

Host Browser (/host)
  → Mic audio → WebSocket → server-rs
  → STT (Deepgram Nova-3) → transcript
  → Per target language:
      → NLLB translate → ElevenLabs TTS → translated audio → guests

Future (FFmpeg RTMP muxing):
  → Host video + translated audio → FFmpeg → RTMP → each platform
```

## Implementation Progress

### Phase 1: YouTube OAuth + Broadcast Creation — DONE

**1.1 Google Cloud Setup** — DONE

- [x] GCP project `brivva-live` created, YouTube Data API v3 enabled
- [x] OAuth2 web credentials configured
- [x] Redirect URI: `https://brivva-server.milliytechnology.org/auth/youtube/callback`

**1.2 Backend: OAuth2 Flow** — DONE (`youtube.rs` + `routes.rs`)

- [x] `GET /auth/youtube?user_id=...` → redirect to Google OAuth consent screen
- [x] `GET /auth/youtube/callback?code=&state=user_id` → exchange code, store tokens, redirect to dashboard
- [x] Token refresh with 60s buffer (`ensure_valid_token`)
- [x] Channel info fetched on OAuth complete

**1.3 Backend: Broadcast Management** — DONE (`routes.rs`)

- [x] `POST /api/sessions` → create session + streams per platform per language
  - YouTube: liveBroadcasts.insert, liveStreams.insert, bind (auto)
  - Others: create_stream_manual with pre-filled RTMP URL + stream key (status "ready")
- [x] `GET /api/sessions?user_id=...` → list sessions
- [x] `GET /api/sessions/:id` → session + streams detail
- [x] `DELETE /api/sessions/:id` → transition YouTube broadcasts to "complete", end session

**1.4 Backend: Stream Management** — DONE

- [x] `POST /api/sessions/:id/streams` → add stream to existing session
- [x] `DELETE /api/sessions/:session_id/streams/:stream_id` → remove stream

**1.5 Backend: Voice Management** — DONE (`routes.rs`)

- [x] `POST /api/voices` → clone via ElevenLabs, save to DB (persistent)
- [x] `GET /api/voices?user_id=...` → list saved voices
- [x] `DELETE /api/voices/:id` → delete from DB + ElevenLabs

**1.6 Backend: Database** — DONE (`db.rs`)

- [x] SQLite via sqlx: users, voices, sessions, streams tables
- [x] Full CRUD for all entities
- [x] Docker volume for persistence
- [x] Streams table generalized: `platform`, `platform_broadcast_id`, `platform_stream_id`, `rtmp_url`, `stream_key`

**1.7 Frontend: Dashboard + Session UI** — DONE

- [x] `api.ts` — REST API client with 13-platform PLATFORMS array (regions, help text, default RTMP URLs)
- [x] `DashboardPage.tsx` — YouTube connect, voice selection, multi-platform session creation
  - Platforms grouped by region: Global, Korea, Japan, China, Other
  - Help text for each platform when enabled
  - Pre-filled RTMP URLs where known
  - Stream key input with password type
- [x] `SessionPage.tsx` — stream cards with platform badges, RTMP URLs, broadcast IDs
- [x] `App.tsx` — /dashboard, /session/:id routes added
- [x] `HomePage.tsx` — "Stream Dashboard" button
- [x] `App.css` — full dashboard + session + platform picker styles

### Phase 2: Video-Audio Sync + FFmpeg RTMP — PARTIALLY DONE

**2.1 Synced Video Buffer Architecture** — DONE (code)

- [x] `types.rs`: TimestampedFrame, FrameBuffer (ring buffer, 10s/300 frames)
- [x] `pipeline.rs`: utterance_start/end tracking, frame extraction
- [x] Frames grabbed matching utterance window when TTS completes

**2.2 FFmpeg RTMP Manager** — DONE (code, NOT WIRED)

- [x] `ffmpeg.rs`: RtmpManager with per-language FFmpeg child processes
- [x] MJPEG stdin → libx264 ultrafast → FLV → RTMP
- [x] SyncedChunk struct bundles frames + MP3 per utterance
- [ ] **NOT WIRED**: RtmpManager not in AppState, not started on session creation
- [ ] **NOT WIRED**: SyncedChunks not pushed from do_tts_and_broadcast
- [ ] Audio muxing not implemented (current FFmpeg uses video-only)
- [ ] FFmpeg binary not in Docker image

**2.3 Remaining to enable live RTMP push**

- [ ] Wire RtmpManager into AppState and session lifecycle
- [ ] Start FFmpeg processes when session goes live
- [ ] Push synced chunks to FFmpeg in do_tts_and_broadcast
- [ ] Handle audio muxing (dual-input FFmpeg: video + audio → RTMP)
- [ ] Add ffmpeg binary to Docker image
- [ ] Test end-to-end RTMP push to YouTube (24hr wait expired ~7:13 PM Mar 21)

### Phase 3: Multi-Platform Support — DONE

**13 Platforms Supported:**

| Platform               | Region | Auto      | Default RTMP                                |
| ---------------------- | ------ | --------- | ------------------------------------------- |
| YouTube                | Global | Yes (API) | Auto-created via YouTube Data API           |
| Instagram              | Global | No        | rtmps://live-upload.instagram.com:443/rtmp/ |
| TikTok                 | Global | No        | Dynamic (from TikTok LIVE Studio)           |
| Twitch                 | Global | No        | rtmp://live.twitch.tv/app/                  |
| Coupang Live           | Korea  | No        | Dynamic (from Coupang Seller Portal)        |
| Naver Shopping Live    | Korea  | No        | Dynamic (from Naver Live Studio)            |
| Rakuten Live           | Japan  | No        | Dynamic (from Rakuten dashboard)            |
| Douyin (抖音)          | China  | No        | Dynamic (from Douyin Live Companion)        |
| Taobao Live (淘宝直播) | China  | No        | Dynamic (from Taobao Live Studio)           |
| Kuaishou (快手)        | China  | No        | rtmp://live.kuaishou.com/live/              |
| Xiaohongshu (小红书)   | China  | No        | Dynamic (from web after app auth)           |
| Bilibili (哔哩哔哩)    | China  | No        | rtmp://live-push.bilivideo.com/live-bvc/    |
| Custom RTMP            | Other  | No        | User-provided                               |

**Implementation:**

- YouTube: full OAuth2 + broadcast auto-creation via API
- All others: user copies RTMP URL + stream key from platform, pastes into dashboard
- Pre-filled RTMP URLs for platforms with known base URLs
- Step-by-step help text for each platform guides non-technical users
- Dashboard groups platforms by region for clean UX

## YouTube API Details

### Quota Budget (default 10,000 units/day)

| Operation                 | Cost | Per Session (4 langs) |
| ------------------------- | ---- | --------------------- |
| liveBroadcasts.insert     | 50   | 200                   |
| liveStreams.insert        | 50   | 200                   |
| liveBroadcasts.bind       | 50   | 200                   |
| liveBroadcasts.transition | 50   | 200                   |
| **Total per session**     |      | **800**               |

12 sessions/day within default quota. Request increase for production.

### Required Scopes

- `https://www.googleapis.com/auth/youtube.force-ssl` — manage broadcasts
- `https://www.googleapis.com/auth/youtube.readonly` — read channel info

### Broadcast Lifecycle

1. `insert` → status: "complete" (metadata ready)
2. `bind` stream to broadcast
3. Start pushing RTMP (FFmpeg) — NOT YET IMPLEMENTED
4. `transition` → "testing" (preview)
5. `transition` → "live" (public)
6. `transition` → "complete" (end stream)

### Known Issues

- **24hr waiting period**: New YouTube channels must wait 24 hours after enabling live streaming before they can actually go live
- **Redirect URI**: Must point to backend (`brivva-server.milliytechnology.org`), not frontend
- **DB schema changes**: Adding `platform` column required deleting old SQLite DB

## Files Changed

```
server-rs/
  Cargo.toml          + sqlx, chrono, reqwest/form — DONE
  Dockerfile          + libsqlite3 (builder + runtime) — DONE
  src/
    main.rs           AppState (rooms + db), all REST + WS routes — DONE
    db.rs             NEW: SQLite init + CRUD (users, voices, sessions, streams) — DONE
    youtube.rs        NEW: OAuth2 + broadcast/stream management — DONE
    routes.rs         NEW: REST API handlers (multi-platform) — DONE
    ffmpeg.rs         NEW: RtmpManager, per-lang FFmpeg processes — DONE (not wired)
    types.rs          + TimestampedFrame, FrameBuffer — DONE
    pipeline.rs       + utterance tracking, synced frame delivery — DONE
    room/handler.rs   + AppState, frame buffer — DONE

docker-compose.yml    + Google env vars, DATABASE_URL, db-data volume — DONE

frontend/
  src/
    lib/
      api.ts              NEW: REST API client, 13 platforms — DONE
    pages/
      DashboardPage.tsx   NEW: multi-platform session creation — DONE
      SessionPage.tsx     NEW: stream cards with platform badges — DONE
      HomePage.tsx        + Dashboard button — DONE
    App.tsx               + /dashboard, /session/:id routes — DONE
    App.css               + dashboard + session + platform styles — DONE
```

## Environment Variables

```env
GOOGLE_CLIENT_ID=...
GOOGLE_CLIENT_SECRET=...
GOOGLE_REDIRECT_URI=https://brivva-server.milliytechnology.org/auth/youtube/callback
DATABASE_URL=sqlite:/data/brivva.db?mode=rwc
ELEVENLABS_API_KEY=...
DEEPGRAM_API_KEY=...
```
