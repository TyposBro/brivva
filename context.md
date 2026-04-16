# Brivva Tech — LLM Collaborator Context (Apr 16, 2026)

## What This Is

**SaaS platform** for real-time multilingual live commerce broadcasting. Host speaks → N platforms receive translated audio + delayed video per language. Competes with Prism Live (broadcast) + Dubly (post-processing dubbing).

**Pivoting from Tauri desktop app → cloud SaaS.** Pipeline moves to AWS Fargate. Auth/billing/dashboard on Cloudflare.

## What Matters — Read This First

**DEAL CONFIRMED Apr 16.** Demo successful — team liked translation quality + default voices. Aziz accepted to team. Technical issues found but all fixable.

**Business model:**

- Charge: $1-2 per OUTPUT minute (not per language, not per source minute)
- Cost: ~$0.1 per output minute (Soniox + ElevenLabs + Fargate ~$0.001/min)
- Margin: ~90%
- 10 existing clients, 200 SOURCE min/mo each, Korean → Chinese guaranteed
- Output min = source min × N target languages (200 source × 3 langs = 600 output min)
- SEA languages (Thai, Vietnamese, Indonesian) = expansion multiplier
- Aziz: 30% profit, Simon+MJ: 70% (B2B sales)
- Baseline (Chinese only): $2,800/mo profit → $840 for Aziz
- At 3 langs avg: $8,400/mo profit → $2,520 for Aziz

**Demo failures (Apr 16) — all must be fixed:**

1. Voice came out 45s before video (A/V sync completely broken)
2. iPhone camera not detected as media input (need capture card or OBS virtual cam)
3. No dedicated camera/mic (hardware gap, client studio will have gear)
4. Cloned voice threw Indian accent in English (voice ID or language detection bug)
5. Only streamed to YouTube, couldn't stream to Grip Live (no RTMPS support)

## SaaS Architecture

```
┌─────────────────────────────────────────────────────────┐
│                    CLOUDFLARE                             │
│                                                           │
│  Pages ─── React Dashboard (login, settings, billing)     │
│  Workers ── Auth API (Google OAuth)                        │
│  Workers ── Session API (create/stop/manage sessions)      │
│  Workers ── Billing API (usage metering, invoicing)        │
│  D1 ─────── Users, sessions, usage records, invoices       │
│  R2 ─────── Voice samples, recordings, dubbed outputs      │
│                                                           │
└──────────────────────┬────────────────────────────────────┘
                       │ HTTPS (session create/stop)
                       ▼
┌─────────────────────────────────────────────────────────┐
│                  AWS FARGATE                               │
│                                                           │
│  ┌─────────────────────────────────┐                      │
│  │  Pipeline Container (per session) │                    │
│  │                                   │                    │
│  │  Browser ──WebSocket──► Rust server                    │
│  │    (audio 0x01, video 0x02)       │                    │
│  │                                   │                    │
│  │  Rust server ──► Soniox WS (STT + translation)        │
│  │             ──► ElevenLabs WS (TTS)                    │
│  │             ──► FFmpeg (per-language RTMP out)          │
│  │                                   │                    │
│  │  Per-language streams:            │                    │
│  │    Source: video+audio (0 delay)  │                    │
│  │    Target: video (delayed) +      │                    │
│  │            host audio 20% +       │                    │
│  │            TTS audio 100%         │                    │
│  └─────────────────────────────────┘                      │
│                                                           │
│  Auto-scale: 0 → N containers based on active sessions    │
│  Scale to zero when idle (no cost)                        │
│                                                           │
└─────────────────────────────────────────────────────────┘
```

### Cloudflare Layer (Auth + Billing + Dashboard)

| Component   | Service    | Purpose                                                          |
| ----------- | ---------- | ---------------------------------------------------------------- |
| Dashboard   | CF Pages   | React SPA — login, session management, billing, voice settings   |
| Auth        | CF Workers | Google OAuth → JWT tokens. User management.                      |
| Session API | CF Workers | Create session → spin up Fargate task. Stop session → kill task. |
| Billing API | CF Workers | Track output minutes per session. Aggregate per client. Invoice. |
| Database    | CF D1      | Users, sessions, usage_records, invoices, voice_configs          |
| Storage     | CF R2      | Voice samples (WAV), session recordings, dubbed outputs          |

### AWS Fargate Layer (Pipeline)

| Component       | Details                                                                                        |
| --------------- | ---------------------------------------------------------------------------------------------- |
| Container image | Rust binary + FFmpeg sidecar. Same code as current Tauri backend.                              |
| Lifecycle       | 1 container per active session. Spun up by CF Workers via AWS SDK. Killed on session end.      |
| Networking      | Public IP for WebSocket (browser → container). Outbound to Soniox, ElevenLabs, RTMP endpoints. |
| Scaling         | ECS Service with desired_count managed by CF Workers. Scale to zero = $0 when idle.            |
| Cost            | ~$0.049/hr per container (1 vCPU, 2GB RAM). ~$0.001/min. Negligible vs API costs.              |
| Logs            | CloudWatch → forward to CF for dashboard.                                                      |

### Data Flow

1. User logs in via Google OAuth (CF Workers)
2. User creates session: selects languages, RTMP destinations, voice settings (CF Dashboard)
3. CF Workers calls AWS ECS RunTask → Fargate container starts (~30s cold start)
4. Container returns WebSocket URL → browser connects
5. Browser sends audio (0x01) + video (0x02) via WebSocket
6. Container processes: STT → Translation → TTS → FFmpeg → RTMP out
7. Container reports usage (output minutes) to CF Workers billing endpoint
8. Session ends → container stops → CF Workers updates billing

### Per-Language Video Delay Model

**Source language stream (Korean → Korean):**

- Video: ZERO delay, passthrough
- Audio: original host voice at 100%, ZERO delay
- Just a rebroadcast — no processing

**Target language streams (Korean → Chinese, Korean → Japanese, etc.):**

- Video: delayed by per-language setting (benchmark: ja=1s, zh=3s, configurable)
- Audio: original host voice at 20% volume (immediate) + TTS at 100% (arrives when ready)
- 1-2s delay between video and translated voice = acceptable for live commerce

**Frontend config per language:**

```json
{
  "languages": [
    { "code": "ko", "type": "source", "delay_ms": 0 },
    {
      "code": "zh",
      "type": "target",
      "delay_ms": 3000,
      "voice_id": "default_zh_female"
    },
    {
      "code": "ja",
      "type": "target",
      "delay_ms": 1000,
      "voice_id": "default_ja_male"
    }
  ]
}
```

## Current Codebase (migrating from desktop → SaaS)

**What stays (Rust pipeline):**

- Soniox v4 STT integration (N+1 connections, semantic endpointing, force chunking)
- ElevenLabs TTS (WebSocket streaming, REST fallback)
- Audio drain (20ms ticks, staleness eviction, jitter recovery)
- FFmpeg RTMP output + crash recovery
- Voice clone API

**What changes:**

- Remove Tauri/desktop shell → standalone Rust HTTP/WS server
- Add per-language video delay (replace global broadcast_delay)
- Add audio mixing (host 20% + TTS 100%)
- Add usage reporting (output minutes → CF billing API)
- Dockerize for Fargate
- Add RTMPS support (TLS for Grip Live etc.)

**What's new (Cloudflare):**

- Google OAuth (CF Workers)
- Dashboard (CF Pages + React)
- Session management API
- Billing/metering (D1 tables: users, sessions, usage_records)
- R2 storage for voice samples + recordings

```
server-rs/src/
├── core/           # Pure types, config, audio utils
├── shared/
│   ├── stt/        # Soniox v4 (10 files)
│   ├── tts/        # ElevenLabs (6 files)
│   ├── dubbing/    # ElevenLabs Dubbing API (Tier 4)
│   ├── recording/  # SessionRecorder
│   └── voice_clone/ # ElevenLabs clone API
├── features/broadcast/
│   ├── domain/     # Session, messages
│   └── data/       # WebSocket handler, RTMP streaming, audio drain
├── orchestration/  # DI, config, router
├── lib.rs          # run_server()
└── main.rs         # Entry point
```

## Soniox v4 Integration

- **WebSocket:** `wss://stt-rt.soniox.com/transcribe-websocket`
- **Model:** `stt-rt-v4`
- **N+1 connections:** 1 source (transcript) + 1 per target language (translation)
- **Semantic endpointing:** Grammar-aware `<end>` token
- **Force chunking:** >4s without endpoint → emit anyway
- **Reconnect:** 5 attempts, 1s delay

## TTS — ElevenLabs

- **Real-time models:** `eleven_turbo_v2_5` (default), `eleven_flash_v2_5` (fast)
- **Tier 4 model:** `eleven_multilingual_v3_enhanced` (80% human, post-processing only)
- **Voice cloning:** Up to 3 min sample. BROKEN in demo (Indian accent — fix needed).
- **Default voices:** Per-language from voice library. Chinese = 75/25 human/robotic.
- **TTFB:** ~75ms (Flash), ~300ms (Turbo)

## Post-Demo Priorities

### Phase 1: Fix Demo Failures (Week 1)

1. Per-language video delay (replace global broadcast_delay)
2. Audio mixing (host 20% + TTS 100%)
3. Source language zero-delay passthrough
4. Voice cloning Indian accent bug
5. RTMPS support for Grip Live

### Phase 2: SaaS Migration (Week 2-3)

1. Strip Tauri shell → standalone Rust server
2. Dockerize pipeline (Rust + FFmpeg)
3. Deploy to Fargate with ECS task definitions
4. CF Workers: Google OAuth + session API
5. CF Pages: React dashboard (login, create session, manage languages)
6. CF D1: users, sessions, usage tables
7. Usage reporting: container → CF billing endpoint

### Phase 3: Production (Week 4+)

1. Per-minute billing integration
2. Client onboarding flow
3. Voice library browser in dashboard
4. Multi-RTMP output per language
5. Tier 4 dubbing from dashboard
6. Monitoring + alerting

## Environment

- **API keys:** `SONIOX_API_KEY`, `TTS_API_KEY` (held server-side, never exposed to client)
- **Build:** `cargo build --release` → Docker image → ECR → Fargate
- **Dev:** `cargo run` or `./dev.sh` (local mode, same as before)
- **CF Dev:** `wrangler dev` for Workers, `npm run dev` for Pages
- **Infra:** Terraform for all AWS resources (ECR, ECS, Fargate, VPC, security groups, IAM, CloudWatch)
- **Architecture rules:** `claude.md` — 4-layer clean architecture

## Cost

| Service        | Per Session | Monthly (100 sessions) |
| -------------- | ----------- | ---------------------- |
| Soniox v4      | ~$0.10      | ~$10                   |
| ElevenLabs TTS | ~$2-5       | ~$200-500              |
| AWS Fargate    | ~$0.03      | ~$3                    |
| Cloudflare     | Free tier   | $0-5                   |
| **Total**      | **~$2-5**   | **~$213-518**          |

## Tone

Grounded, direct, technical. Ship quality, not features. When in doubt, ask — don't guess.
