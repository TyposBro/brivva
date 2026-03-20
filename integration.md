# Multi-Platform Live Stream Integration

## Overview

Host signs up → connects YouTube account (optional) → selects voice → creates session with title + target language + platform + stream key → Brivva creates 1 FFmpeg RTMP stream → host talks → that stream gets real-time translated audio + host video pushed to the platform.

## Fundamental Challenge: 1 Stream = 1 Account = 1 Language

**RTMP platforms only allow ONE ingest stream per account.** Twitch, Instagram, TikTok, etc. all enforce this. You cannot push 3 languages to the same stream key — the 2nd and 3rd connections will be rejected.

### What This Means for Multi-Language Distribution

To stream Korean host → English + Japanese + Chinese simultaneously:
- **3 Twitch accounts** (twitch.tv/brand_en, twitch.tv/brand_ja, twitch.tv/brand_zh), each with its own stream key
- **OR 3 different platforms** (EN → Twitch, JA → YouTube, ZH → Bilibili)
- **YouTube is the exception** — OAuth API can auto-create multiple broadcasts on one account

### How Competitors Solve This

- **Firework, Bambuser, etc.**: Single embedded player on brand's website, language switcher in player UI. No RTMP distribution — they host the player.
- **Restream, Castr**: Multi-platform RTMP relay — still 1 language per account, user provides N stream keys
- **No one** does real-time multi-language RTMP distribution to native platform players yet. This is Brivva's unique value prop.

### Implementation Phases

| Phase | Scope | Status |
|-------|-------|--------|
| **Phase 1** | 1 session → 1 stream → 1 platform account → 1 language | **In progress** |
| **Phase 2** | 1 session → N streams, each with own platform/account/language | Next |
| **Phase 3** | YouTube auto-creates N broadcasts per language on 1 account via API | YouTube API approved |

### Phase 1 Model (Current Focus)

```
Dashboard: Pick 1 platform, 1 language, paste 1 stream key
  → Session has 1 stream record in DB
  → Host clicks "Start Broadcasting" → /host?sessionId=xxx
  → Server reads stream from DB → starts 1 FFmpeg process
  → FFmpeg: host video (stdin) + translated TTS audio (FIFO) → RTMP push
  → Platform shows live stream with translated audio
```

### Phase 2 Model (Next)

```
Dashboard: For each target language, assign a platform + stream key
  EN → Twitch (stream key A)
  JA → YouTube (auto-created)
  ZH → Bilibili (stream key B)
  → Session has N stream records in DB
  → Server starts N FFmpeg processes, each with different RTMP URL
  → Each FFmpeg gets same video but different TTS audio (per language)
```

## Architecture (v9)

```
Host Browser (/dashboard)
  → Connect YouTube (OAuth2)
  → Create session: title, source lang, target lang, platform, stream key
  → Session page shows stream card

Host Browser (/host?sessionId=xxx)
  → Webcam 30fps JPEG → face:frame → server-rs → FFmpeg stdin (all streams)
  → Mic audio → WebSocket → server-rs
  → STT (Deepgram Nova-3) → transcript
  → NLLB translate → ElevenLabs TTS → MP3
  → MP3 → decode to PCM → FFmpeg audio FIFO (per-language stream)
  → FFmpeg: H.264 + AAC → FLV → RTMP push to platform

FFmpeg per stream:
  Video: -f image2pipe -framerate 30 -i pipe:0
  Audio: -f s16le -ar 44100 -ac 1 -i /tmp/brivva_audio_{stream_id}
  Output: -c:v libx264 -preset ultrafast -tune zerolatency -c:a aac -f flv rtmp://...
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

- [x] `POST /api/sessions` → create session + stream per platform
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

- [x] SQLite via sqlx: users, voices, sessions, streams, platform_credentials tables
- [x] Full CRUD for all entities
- [x] Docker volume for persistence

**1.7 Frontend: Dashboard + Session UI** — DONE

- [x] `DashboardPage.tsx` — YouTube connect, voice selection, platform picker
- [x] `SessionPage.tsx` — stream cards with platform badges, RTMP URLs
- [x] `HostPage.tsx` — stream status cards, webcam, audio recorder (redesigned v9)
- [x] Zero-config UX: magic paste, credential vault, key-only mode, deep links

### Phase 2: FFmpeg RTMP Streaming — DONE (code wired, needs 1:1 fix)

**2.1 FFmpeg Manager** — DONE (`ffmpeg.rs`)

- [x] RtmpManager with per-stream FFmpeg child processes
- [x] Dual-input: video via stdin (image2pipe JPEG 30fps), audio via named FIFO (s16le 44100Hz)
- [x] Silence padding fills gaps between TTS utterances (20ms zero chunks)
- [x] decode_mp3_to_pcm() for TTS MP3 → raw PCM conversion
- [x] FFmpeg binary in Docker image

**2.2 Session-Room Linking** — DONE (`handler.rs`)

- [x] Host connects with `?sessionId=xxx` → handler reads streams from DB
- [x] Starts FFmpeg per stream, stores RtmpManager in Room
- [x] Updates session status to "live" with room_id
- [x] On disconnect: stops all FFmpeg, updates session to "ended"

**2.3 Pipeline Integration** — DONE (`pipeline.rs` + `handler.rs`)

- [x] face:frame → decoded JPEG → push to all FFmpeg video stdin
- [x] TTS MP3 → decode to PCM → push to language-matched FFmpeg audio FIFO
- [x] DashMap borrow safety: clone Arc before await

**2.4 Remaining Issues**

- [ ] **1:1 stream model**: Dashboard creates duplicate streams (same key, all languages). Must create exactly 1 stream per unique RTMP endpoint.
- [ ] **Audio-video sync**: TTS audio arrives in bursts (3-5s) but video is continuous 30fps. Utterances need timestamps for correct placement.

### Phase 3: Multi-Platform Support — DONE (13 platforms configured)

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

### Platform Partnership Requirements

| Platform | Requirement | Why |
|----------|-------------|-----|
| Coupang Live | Korean business registration + Coupang Wing seller account + partnership | No open API, seller-only access |
| Naver Shopping Live | Korean business registration + Naver Smart Store seller account | No open API |
| Rakuten Live | Japanese business registration + Rakuten seller account | No open API |
| Douyin, Taobao, Kuaishou, Xiaohongshu, Bilibili | Chinese business registration + individual platform partnerships | Each requires separate approval |

## YouTube API Details

### Quota Budget (default 10,000 units/day)

| Operation                 | Cost | Per Session |
| ------------------------- | ---- | ----------- |
| liveBroadcasts.insert     | 50   | 50          |
| liveStreams.insert         | 50   | 50          |
| liveBroadcasts.bind       | 50   | 50          |
| liveBroadcasts.transition | 50   | 50          |
| **Total per session**     |      | **200**     |

50 sessions/day within default quota. Request increase for production.

### Broadcast Lifecycle

1. `insert` → status: "complete" (metadata ready)
2. `bind` stream to broadcast
3. Start pushing RTMP (FFmpeg)
4. `transition` → "testing" (preview)
5. `transition` → "live" (public)
6. `transition` → "complete" (end stream)

### Known Issues

- **24hr waiting period**: New YouTube channels must wait 24 hours after enabling live streaming before they can actually go live
- **Redirect URI**: Must point to backend (`brivva-server.milliytechnology.org`), not frontend

## Environment Variables

```env
GOOGLE_CLIENT_ID=...
GOOGLE_CLIENT_SECRET=...
GOOGLE_REDIRECT_URI=https://brivva-server.milliytechnology.org/auth/youtube/callback
DATABASE_URL=sqlite:/data/brivva.db?mode=rwc
ELEVENLABS_API_KEY=...
DEEPGRAM_API_KEY=...
```
