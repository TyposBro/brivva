# Multi-Platform Live Stream Integration

## Overview

Host signs up → connects YouTube account (optional) → selects voice → creates session with destinations (platform + language + stream key per destination) → Brivva creates N FFmpeg RTMP streams → host talks → source-language streams get passthrough audio (host's actual voice), other streams get translated TTS audio + host video pushed to each platform.

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

## Architecture (v12)

```
Host Browser (/dashboard)
  → Connect YouTube (OAuth2, settings drawer)
  → Create session: title, source lang, N destinations (platform + lang + RTMP)
  → Progressive disclosure: collapsible platform picker grouped by region
  → Platform-language auto-mapping (Coupang→ko, Rakuten→ja, Twitch→en)

Host Browser (/host?sessionId=xxx)
  → Camera capture at native resolution (up to 4K) → JPEG → face:frame → server-rs
  → Mic audio 44.1kHz PCM → WebSocket → server-rs
  → server-rs routes audio to:
    - STT (Deepgram Nova-3, 44.1kHz) → transcript
    - Source-lang RTMP streams: passthrough (host's raw audio, no TTS)
  → For other languages:
    - Google Cloud Translation API v2 → ElevenLabs TTS → MP3 → decode to PCM
    - Queue PCM to language-matched FFmpeg audio FIFO
  → FFmpeg per stream: H.264 CRF 20 + AAC → FLV → RTMP push

FFmpeg per stream:
  Video: -f image2pipe -framerate 30 -i pipe:0
  Audio: -f s16le -ar 44100 -ac 1 -i /tmp/brivva_audio_{stream_id}
  Encode: -c:v libx264 -crf 20 -maxrate 35000k -preset ultrafast -tune zerolatency
  Output: -c:a aac -ac 2 -b:a 128k -f flv rtmp://...
```

### Audio Routing

| Stream Language | Path | Latency | API Cost |
|----------------|------|---------|----------|
| Same as host (passthrough) | Raw 44.1kHz PCM → queue directly | ~0ms processing | None |
| Different from host | STT → Google Translate → TTS → decode → queue | ~1-3s | Deepgram + Google Translate + ElevenLabs |

### Estimated Cost Per Stream (2-hour session, 4 utterances/min)

Assumptions: ~15 words/utterance, ~5 chars/word, 480 utterances per 2-hour stream

| Service | Cost/Stream | Calculation | Pricing |
|---------|-------------|-------------|---------|
| Deepgram STT | $0.52 | 120 min × $0.0043/min | $0.0043/min (Nova-3 Pay-as-you-go) |
| Google Translate | $0.72 | 36K chars × $20/M chars | $20/M chars (first 500K/mo free) |
| ElevenLabs TTS | ~$0.00 | Included in plan | Scale plan: $99/mo for 2M chars |
| **Total per stream** | **$1.24** | | After free tier; $0.52 within free tier |

### Monthly Cost Projections

| Streams/Month | Deepgram | Google Translate | ElevenLabs | AWS EC2 | Total |
|---------------|----------|-----------------|------------|---------|-------|
| 5 (starter) | $2.60 | $0 (free tier) | $5 (starter) | $30 | **~$38/mo** |
| 30 (growing) | $15.60 | $7.80 | $22 (creator) | $30 | **~$75/mo** |
| 100 (scale) | $52 | $52 | $99 (scale) | $30 | **~$233/mo** |
| 500 (enterprise) | $260 | $260 | $330 (scale+) | $60 (t3.large) | **~$910/mo** |

### Infrastructure Cost Comparison

| Component | Old (v11 + GPU) | New (v12, API-only) | Savings |
|-----------|----------------|---------------------|---------|
| AWS EC2 | g5.xlarge $750/mo | t3.medium $30/mo | **96% reduction** |
| Docker containers | 3 (server, stt, nllb) | 2 (server, stt) | Simpler stack |
| GPU | Required (NLLB) | Not needed | No GPU management |
| Translation latency | NLLB 80-160ms (local GPU) | Google API ~40ms (Seoul region) | **2-4x faster** |
| Break-even vs GPU | — | ~600 streams/mo | GPU only wins at extreme scale |

## Implementation Progress

### Phase 1: YouTube OAuth + Broadcast Creation — DONE

**1.1 Google Cloud Setup** — DONE

- [x] GCP project `brivva-live` created, YouTube Data API v3 + Cloud Translation API enabled
- [x] OAuth2 web credentials configured (scope: `youtube` + `youtube.readonly`)
- [x] Redirect URI: `https://brivva-server.milliytechnology.org/auth/youtube/callback`
- [x] Google Search Console domain verification via meta tag

**1.2 Backend: OAuth2 Flow** — DONE (`youtube.rs` + `routes.rs`)

- [x] `GET /auth/youtube?user_id=...` → redirect to Google OAuth consent screen
- [x] `GET /auth/youtube/callback?code=&state=user_id` → exchange code, store tokens, redirect to dashboard
- [x] Token refresh with 60s buffer (`ensure_valid_token`)
- [x] Channel info fetched on OAuth complete

**1.3 Backend: Broadcast Management** — DONE (`routes.rs`)

- [x] `POST /api/sessions` → create session + one stream per platform destination
  - YouTube: liveBroadcasts.insert, liveStreams.insert, bind (auto) — one per destination
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

- [x] `DashboardPage.tsx` — Notion-style progressive disclosure: collapsible platform picker by region, expandable destination cards, settings drawer for YouTube/voices
- [x] `SessionPage.tsx` — stream cards with platform badges, RTMP URLs, status
- [x] `HostPage.tsx` — stream status cards, webcam preview, audio recorder, latency dashboard
- [x] `PrivacyPage.tsx` + `TermsPage.tsx` — for Google OAuth verification
- [x] Zero-config UX: magic paste, credential vault, key-only mode, deep links
- [x] Platform-language auto-mapping prevents nonsensical combos (e.g., English on Coupang)

### Phase 2: FFmpeg RTMP Streaming — DONE

**2.1 FFmpeg Manager** — DONE (`ffmpeg.rs`)

- [x] RtmpManager with per-stream FFmpeg child processes
- [x] Dual-input: video via stdin (image2pipe JPEG, native resolution up to 4K), audio via named FIFO (s16le 44100Hz)
- [x] CRF 20 encoding with 35Mbps maxrate — auto-adapts quality to any input resolution
- [x] Silence padding fills gaps between TTS utterances (20ms zero chunks)
- [x] decode_mp3_to_pcm() for TTS MP3 → raw PCM conversion
- [x] Dedicated OS threads for video drain (30fps) and audio drain (20ms ticks)
- [x] Graceful shutdown with 3s join timeout + startup orphan sweep
- [x] FFmpeg binary in Docker image

**2.2 Session-Room Linking** — DONE (`handler.rs`)

- [x] Host connects with `?sessionId=xxx` → handler reads streams from DB
- [x] Starts FFmpeg per stream, stores RtmpManager in Room
- [x] Updates session status to "live" with room_id
- [x] On disconnect: stops all FFmpeg, updates session to "ended"

**2.3 Pipeline Integration** — DONE (`pipeline.rs` + `handler.rs`)

- [x] face:frame → decoded JPEG → push to all FFmpeg video stdin (native resolution)
- [x] Source-lang streams: host audio accumulated per utterance, queued directly (passthrough)
- [x] Other langs: Google Translate → TTS MP3 → decode to PCM → push to language-matched FFmpeg audio FIFO
- [x] DashMap borrow safety: clone Arc before await

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
DEEPGRAM_API_KEY=...
ELEVENLABS_API_KEY=...
GOOGLE_TRANSLATE_API_KEY=...
GOOGLE_CLIENT_ID=...
GOOGLE_CLIENT_SECRET=...
GOOGLE_REDIRECT_URI=https://brivva-server.milliytechnology.org/auth/youtube/callback
DATABASE_URL=sqlite:/data/brivva.db?mode=rwc
BROADCAST_DELAY_MS=2500
```
