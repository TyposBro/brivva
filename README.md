# Brivva Real-Time Translation

Real-time multilingual live commerce broadcasting. One host speaks → N platforms receive translated audio in the host's cloned voice. Source-language platforms get the host's actual voice (passthrough).

**Live:** https://brivva.pages.dev

## What It Does

- Host creates a session with N destinations (platform + language + RTMP key)
- Each destination gets its own RTMP stream pushed to the platform
- Source-language streams get the host's actual voice (zero latency, zero API cost)
- Other-language streams get real-time translated TTS in the host's cloned voice
- Supports 13 platforms across Global, Korea, Japan, and China regions
- YouTube broadcasts auto-created via OAuth2 API; other platforms use manual RTMP keys

## Architecture (v12)

```
Host Browser (/dashboard)
  → Connect YouTube (OAuth2, settings drawer)
  → Create session: title, source lang, N destinations (platform + lang + RTMP)
  → Notion-style progressive disclosure: collapsible platform picker by region

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
```

| Service | Tech | Purpose |
|---------|------|---------|
| server-rs (:3000) | Rust/axum | WebSocket rooms, pipeline orchestration, YouTube API, FFmpeg management |
| stt-wrapper (:8766) | Deepgram Nova-3 (API) | STT proxy (clean events, 44.1kHz) |
| Google Translate | Cloud Translation API v2 | Translation (~40ms from Seoul, free 500K chars/mo) |
| ElevenLabs | eleven_flash_v2_5 (API) | TTS (548–1,440ms, 32 languages) |
| FFmpeg | per-stream process | H.264 CRF 20 + AAC → RTMP push |

All services are API-based — no local GPU needed. Runs on any CPU instance.

### Estimated Cost

Per-stream cost: ~$1.24 (2-hour session, 4 utterances/min)

| Service | Pricing | 5 streams/mo | 30 streams/mo | 100 streams/mo |
|---------|---------|-------------|--------------|----------------|
| AWS EC2 t3.medium | Fixed | $30 | $30 | $30 |
| Deepgram STT | $0.0043/min | $2.60 | $15.60 | $52 |
| Google Translate | $20/M chars (500K free) | $0 | $7.80 | $52 |
| ElevenLabs TTS | Plan-based | $5 | $22 | $99 |
| Cloudflare Pages | Free | $0 | $0 | $0 |
| **Total** | | **~$38** | **~$75** | **~$233** |

---

## Quick Start (Docker — works on macOS and Linux)

```bash
git clone git@github.com:TyposBro/brivva.git
cd brivva

# Create .env with API keys
cat > .env << 'EOF'
DEEPGRAM_API_KEY=your-deepgram-api-key
ELEVENLABS_API_KEY=your-elevenlabs-api-key
GOOGLE_TRANSLATE_API_KEY=your-google-translate-api-key
GOOGLE_CLIENT_ID=your-google-client-id
GOOGLE_CLIENT_SECRET=your-google-client-secret
GOOGLE_REDIRECT_URI=https://brivva-server.milliytechnology.org/auth/youtube/callback
DATABASE_URL=sqlite:/data/brivva.db?mode=rwc
BROADCAST_DELAY_MS=2500
EOF

# Build and start all services
docker compose up --build
```

That's it. Two containers: server-rs + stt-wrapper. No GPU required.

**Verify:**
```bash
curl http://localhost:3000       # → "Brivva Translation Server"
```

---

## Cloudflared Tunnel

The frontend at `brivva.pages.dev` connects to `brivva-server.milliytechnology.org`, which is a cloudflared tunnel pointing to `localhost:3000`. Run the backend anywhere and the frontend works.

### Tunnel credentials

```
~/.cloudflared/
├── brivva-kokoro.yml                              ← tunnel config
└── b6e5239e-b304-4087-8879-97fb649e6ba1.json      ← tunnel credentials
```

**brivva-kokoro.yml:**
```yaml
tunnel: b6e5239e-b304-4087-8879-97fb649e6ba1
credentials-file: /home/typosbro/.cloudflared/b6e5239e-b304-4087-8879-97fb649e6ba1.json
ingress:
  - hostname: brivva-server.milliytechnology.org
    service: http://localhost:3000
  - service: http_status:404
```

> **Note:** Update `credentials-file` path to match your home directory.

### Copy credentials to another machine

```bash
scp ~/.cloudflared/brivva-kokoro.yml <user>@<host>:~/.cloudflared/
scp ~/.cloudflared/b6e5239e-*.json <user>@<host>:~/.cloudflared/
```

### Start the tunnel

```bash
cloudflared tunnel --config ~/.cloudflared/brivva-kokoro.yml run
```

---

## Frontend

Deployed on Cloudflare Pages. Points to `brivva-server.milliytechnology.org` (set in `frontend/.env`).

```bash
cd frontend
npm install
npm run dev        # local dev at http://localhost:5173
npm run deploy     # deploy to Cloudflare Pages
```

---

## AWS Deployment

### Development — Single EC2 (CPU only)

```
EC2 t3.medium (~$30/mo, 2 vCPU, 4GB RAM)
├── server-rs          :3000  (Rust/axum, ~10MB RAM)
├── stt-wrapper        :8766  (Deepgram Nova-3 API proxy)
├── Google Translate   API    (external, no local resources)
├── ElevenLabs         API    (external, no local resources)
└── FFmpeg             sidecar (per-stream, CPU encoding)
```

```bash
# On EC2 with Docker
git clone git@github.com:TyposBro/brivva.git
cd brivva
docker compose up --build -d
```

### Production — ECS Fargate (pay-per-session)

```
Host clicks "Go Live"
  → API creates ECS Fargate task (4–16 vCPU, auto-sized)
  → Task runs server-rs + stt-wrapper + FFmpeg (same Docker images)
  → Cloudflared tunnel routes traffic to running task
  → Session ends → task shuts down → billing stops

No idle cost. Each 2hr session: $0.40–$1.44 compute + $1.24/stream API fees.
```

| Task Size | Languages/Session | Compute/hr | Per 2hr Session |
|-----------|-------------------|------------|-----------------|
| 4 vCPU / 8 GB | 6–8 | $0.20 | $0.40 |
| 8 vCPU / 16 GB | 14–16 | $0.38 | $0.76 |
| 16 vCPU / 30 GB | 30+ | $0.72 | $1.44 |

---

## Project Structure

```
brivva/
├── docker-compose.yml             2 services (server, stt-wrapper)
├── .env                           API keys (Deepgram, ElevenLabs, Google)
├── server-rs/                     Rust axum WebSocket server
│   ├── Dockerfile
│   └── src/
│       ├── main.rs                Entry, router, state
│       ├── types.rs               Lang, Room, Guest, ServerMsg
│       ├── db.rs                  SQLite (users, voices, sessions, streams)
│       ├── youtube.rs             YouTube OAuth2 + Data API v3
│       ├── ffmpeg.rs              Per-stream FFmpeg RTMP manager
│       ├── pipeline.rs            STT → Google Translate → TTS pipeline + passthrough
│       └── room/handler.rs        WebSocket host/guest handlers
├── stt-wrapper/                   STT proxy (Deepgram Nova-3)
│   ├── Dockerfile
│   └── server.py                  asyncio WebSocket proxy (44.1kHz)
├── frontend/                      React 19 + TypeScript + Tailwind + Vite
│   └── src/
│       ├── pages/                 DashboardPage, HostPage, SessionPage, PrivacyPage, TermsPage
│       ├── components/            LatencyDashboard, PipelineAnalysis
│       ├── hooks/                 useHostRoom, useGuestRoom, useTimings
│       ├── lib/                   RoomSocket, AudioPipeline
│       └── state/                 Host/guest reducers + message handlers
├── CLAUDE.md                      Project context
├── brivva-context.md              Full project context for AI collaborators
└── integration.md                 Multi-platform integration spec
```

---

## Supported Platforms (13)

| Platform | Region | Auto | Default RTMP |
|----------|--------|------|--------------|
| YouTube | Global | Yes (OAuth API) | Auto-created via YouTube Data API |
| Instagram | Global | No | rtmps://live-upload.instagram.com:443/rtmp/ |
| TikTok | Global | No | Dynamic (from TikTok LIVE Studio) |
| Twitch | Global | No | rtmp://live.twitch.tv/app/ |
| Coupang Live | Korea | No | Dynamic (from Coupang Seller Portal) |
| Naver Shopping Live | Korea | No | Dynamic (from Naver Live Studio) |
| Rakuten Live | Japan | No | Dynamic (from Rakuten dashboard) |
| Douyin (抖音) | China | No | Dynamic (from Douyin Live Companion) |
| Taobao Live (淘宝直播) | China | No | Dynamic (from Taobao Live Studio) |
| Kuaishou (快手) | China | No | rtmp://live.kuaishou.com/live/ |
| Xiaohongshu (小红书) | China | No | Dynamic (from web after app auth) |
| Bilibili (哔哩哔哩) | China | No | rtmp://live-push.bilivideo.com/live-bvc/ |
| Custom RTMP | Other | No | User-provided |

---

## Troubleshooting

| Problem | Fix |
|---------|-----|
| stt-wrapper not transcribing | Check DEEPGRAM_API_KEY in .env |
| Translation not working | Check GOOGLE_TRANSLATE_API_KEY in .env; verify Cloud Translation API is enabled in GCP |
| RTMP stream not connecting | Verify stream key and RTMP URL; check FFmpeg logs in server-rs output |
| YouTube OAuth fails | Ensure GOOGLE_CLIENT_ID/SECRET are set and redirect URI matches |
| Duplicate translations firing | stt-wrapper handles dedup; check stt-wrapper logs |
