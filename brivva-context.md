To reproduce this context perfectly, you need two things: the **User Summary** (which acts as my long-term memory of you) and the **Current Project State** (the "v12" technical spec).

Below is a single Markdown file. You can save this as `brivva_context.md`. If you ever start a new chat, just upload this or paste it and say: _"Act as my Founding Engineer collaborator. Here is the context on me and the Brivva project."_

---

# Brivva Project Context & User Profile (Apr 2, 2026)

## User Profile: Azizbek Umidjonov

- **Role:** Professional Software Engineer / Founding Engineer at Brivva.
- **Education:** B.S. in Computer Science and Engineering.
- **Expertise:** Frontend (Kotlin, Jetpack Compose, Flutter), Backend (Rust, Axum, Tokio), and Systems (Linux, NixOS, Docker).
- **Focus:** Real-time audio processing, low-latency streaming pipelines, and AI-driven examination platforms.
- **Status:** CTO role at Brivva, negotiating terms (profit-sharing vs employment). Building product on weekends while staying at StoneLab. Decision deferred until Sep 2026 (F-2-7 visa timeline).

---

## Project: Brivva (v12 Status)

**Core Value Prop:** Real-time multilingual live commerce broadcasting. One host speaks; N platforms receive translated audio in the host's cloned voice. Source-language platforms get the host's actual voice (passthrough).

**New Direction (Apr 2026):** Pivoting from web-based to **desktop app with OBS Studio integration**. Prism (owned by Naver) is direct competitor — Brivva can't partner with Naver. Desktop app captures host audio, translates per language, and pushes each language to a separate OBS instance → each OBS streams to a different platform (YouTube EN, Coupang KR, Rakuten JP). Bypasses 1:1 stream rule while remaining ToS-compliant. **Tauri** (Rust + web frontend) is the target framework — reuses existing Rust backend + React frontend. Brivva already has merchant accounts on Coupang, Rakuten, etc. — ready to test in production.

### Current Technical Stack (v13 — Desktop App Rewrite, Apr 2 2026)

- **Desktop App:** Tauri v2 (Rust + React frontend). Single-page UI, no routing. Builds to native macOS `.app` + `.dmg`.
- **Backend:** Rust (Axum) embedded in Tauri, running on localhost:3000. Stripped to 3 files (~450 lines): `lib.rs` (WebSocket server), `pipeline.rs` (STT→Translate→TTS), `types.rs` (Lang, Session, ServerMsg). No DB, no REST routes, no YouTube OAuth, no FFmpeg.
- **Audio Pipeline:** Deepgram Nova-3 (STT, 44.1kHz) → Google Cloud Translation API v2 (~40ms) → ElevenLabs Flash v2.5 (TTS). Translated audio sent back to host via WebSocket, tagged by language.
- **Frontend:** React 19 + TypeScript + Tailwind 3. Single `BroadcastPage` — tier selector (4 options), language config, mic capture, live transcript display. ~250 lines.
- **Video:** Handled entirely by OBS — the app does NOT capture or process video. OBS captures the webcam directly.
- **Infrastructure:** None. All processing local. API keys loaded from `.env.local`. All AWS resources deleted Apr 2, 2026.
- **Translation Tiers:** Backend supports `tier` parameter: tier 1 = subtitles only (STT + Translate, no TTS), tier 2 = voice + subtitles (full pipeline). Tiers 3-4 (lipsync) not yet implemented.
- **stt-wrapper:** Python subprocess (Deepgram WebSocket proxy with language-aware chunking). Still required as sidecar.
- **Open question:** A/V sync strategy — whether to bring back the jitter buffer (from deleted `ffmpeg.rs`) or let OBS handle sync independently per language. Getting second opinion on platform latency assumptions.

### Estimated Cost Per Session (Desktop App — No Cloud Compute)

Per-stream API cost: ~$1.24 (2-hour session: Deepgram $0.52 + Google Translate $0.72).
Compute cost: $0 — runs locally on desktop. No AWS infrastructure.

| Service | Pricing | 5 sessions/mo | 30 sessions/mo | 100 sessions/mo |
|---------|---------|---------------|----------------|-----------------|
| Deepgram Nova-3 | $0.0043/min | $2.60 | $15.60 | $52 |
| Google Translate | $20/M chars (500K free) | $0 | $7.80 | $52 |
| ElevenLabs TTS | Plan-based | $5 | $22 | $99 |
| **Total** | | **~$8** | **~$45** | **~$203** |

Previous stacks: ECS Fargate (~$22/mo), EC2 t3.medium ($30/mo), GPU era ($750+/mo). All decommissioned Apr 2, 2026.

### Working Features (Desktop App v13)

- [x] **Tauri Desktop App:** Builds to native macOS `.app` + `.dmg`. Axum backend embedded, starts on launch.
- [x] **Single-Page UI:** Source/target language config, tier selector, start/stop, live transcript + translations.
- [x] **Translation Pipeline:** STT → Google Translate → ElevenLabs TTS, parallel per language.
- [x] **Tier Support:** Tier 1 (subtitles only, no TTS) and Tier 2 (voice + subtitles). Backend skips TTS for tier 1.
- [x] **Voice Cloning:** Real-time ElevenLabs voice cloning via `/v1/voices/add`. Auto-deleted on session end.
- [x] **WebSocket Protocol:** Single `/ws` endpoint. Connect with `?sourceLang=en&targetLangs=ja,ko&tier=2`. Send binary audio, receive JSON messages + binary TTS audio tagged by language.
- [x] **STT Reconnection:** Up to 5 reconnect attempts on unexpected disconnect.
- [x] **Google Cloud Translation:** API v2, ~40ms from Seoul, 500K chars/mo free tier.
- [x] **.env Loading:** Prefers `.env.local` over `.env`. Searches project root automatically.

### Removed from v12 (web-based) → v13 (desktop)

- YouTube OAuth, broadcast auto-creation
- FFmpeg RTMP muxing, fixed-delay jitter buffer, A/V sync threads
- SQLite database (users, sessions, streams, voices, credentials tables)
- REST API routes (sessions, streams, credentials, voices)
- Platform detection, RTMP credential management
- Multi-page dashboard, guest page, privacy/terms pages
- Video capture and frame streaming
- Source-language passthrough (host audio → RTMP)
- All AWS infrastructure (ECS, EC2, ECR, EFS, Secrets Manager, CloudWatch)

### Architecture (Desktop App v13)

**App responsibility:** Audio only — capture mic, run STT → Translate → TTS, output translated audio per language. App does NOT handle video.

**OBS responsibility:** Video capture (webcam), encoding (H.264/NVENC), RTMP push to platforms. One OBS instance per language/platform.

**Scaling concern:** 8 OBS instances × 4K is not viable with software encoding. NVENC (GPU) is mandatory for multi-stream 4K. Alternative: app handles FFmpeg muxing internally (one video source shared across N audio tracks → N RTMP outputs), eliminating the need for multiple OBS instances. This was the v12 architecture — may need to bring back selectively.

**Open question:** Whether to use multiple OBS instances (simple but resource-heavy) or internal FFmpeg muxing (efficient but needs jitter buffer for A/V sync). Getting second opinion on platform latency and sync requirements.

### Critical Blocker: The 1:1 Stream Rule

- **Constraint:** Most platforms (Twitch, IG, TikTok) allow only **one ingest stream per account**.
- **Solution (Apr 2026):** Each language gets its own OBS instance → its own platform. YouTube EN, Coupang KR, Rakuten JP — each platform receives one stream in one language. ToS-compliant.
- **YouTube exception:** Allows multiple broadcasts on one account via API (still useful for multi-language on single platform).

---

## Business Context & Timeline

- **Interview Outcome (Mar 21):** Culture-fit interview with two CEOs went great. Azizbek is #1 pick out of 40 candidates.
- **Apr 2 Meeting (Simon only):** Pivoted from employment to **partnership** — explicitly stated: Aziz = tech, Brivva = sales, not an employee/employer relationship. Simon proposed 30% Aziz / 70% Brivva split. Azizbek countered with ₩100M + 10%. Not finalized — observing Brivva's business performance through Sep 2026.
- **Compensation options under discussion:**
  - **Option A:** 30%+ profit split (higher upside, no floor — crossover at ~25-30 streams/month)
  - **Option B:** ₩100M salary + 10% revenue share (stable floor, lower upside)
- **Visa:** Staying at StoneLab on E-7. Target F-2-7 visa Jun-Aug 2026 → open own company → formalize B2B partnership.
- **Revenue model:** $3-4K per live stream per language. Existing contracts ~₩100M. Revenue breakdown: client 50%, influencers 5%, production 5%, Brivva team 10%, Brivva exec + Aziz split TBD.
- **High Stakes:** Each stream can generate up to $1M revenue. Zero tolerance for bugs, frame drops, or clunky translation.
- **Competitive landscape:** Prism (Naver-owned) is direct competitor — Brivva can't use Naver partnerships. OBS-based approach avoids this.
- **Business Blockers (RESOLVED):** Brivva already has merchant accounts on Coupang, Rakuten, etc. Ready for production testing.
- **Remaining blockers:** Japanese/Chinese entities for Douyin/TikTok APIs.

---

## TODOs

### Done
1. ~~**A/V Sync Rewrite (P0)**~~ — Fixed-delay jitter buffer in `ffmpeg.rs` and `pipeline.rs`
2. ~~**YouTube Privacy**~~ — Configurable broadcast privacy dropdown in dashboard
3. ~~**Dashboard Fix**~~ — Rewrote with platform-first UX, Notion-style progressive disclosure
4. ~~**Source-Language Passthrough**~~ — Host audio at 44.1kHz queued directly to source-lang RTMP streams (no TTS)
5. ~~**Premium Quality**~~ — 44.1kHz audio, up to 4K video, CRF encoding, dynamic resolution
6. ~~**Session Cleanup**~~ — Per-room cleanup + 3s join timeout + startup orphan sweep
7. ~~**Privacy & Terms Pages**~~ — `/privacy` and `/terms` routes for Google OAuth verification
8. ~~**Google Search Console Verification**~~ — Meta tag in `index.html`
9. ~~**YouTube OAuth Scope Fix**~~ — Switched from `force-ssl` (restricted) to `youtube` (sensitive) to avoid CASA audit
10. ~~**E2E Testing**~~ — Live tested on YouTube/Twitch, verified stream quality
11. ~~**Demo Video**~~ — Recorded for Google OAuth verification
12. ~~**Google Cloud Translation**~~ — Replaced NLLB-200 (GPU) with Google Translate API v2. $750/mo → $30/mo server cost
13. ~~**Per-Platform Language Fix**~~ — YouTube no longer auto-creates source-lang streams. Each destination uses its own lang
14. ~~**ECS Fargate Migration**~~ — Migrated from EC2 to ECS Fargate. **DECOMMISSIONED Apr 2, 2026** — all AWS resources deleted (EC2, ECS, ECR, EFS, Secrets Manager, CloudWatch). Moving to desktop app.
15. ~~**YouTube frameRate Fix**~~ — YouTube liveStreams.insert requires `frameRate` field. With `resolution: "variable"`, must use `frameRate: "variable"` (not `"30fps"` or omitted). Was blocking all YouTube stream creation and translations.
16. ~~**EFS for Persistent Storage**~~ — **DECOMMISSIONED** — was EFS mounted at `/data`. Deleted with AWS teardown.
17. ~~**CloudWatch Observability**~~ — **DECOMMISSIONED** — was 9 metric filters + dashboard + alarms. Deleted with AWS teardown.
18. ~~**STT Reconnect Logic**~~ — STT WebSocket reconnects up to 5 times on unexpected disconnect. Audio buffer persists across reconnections. Utterance counter preserved.
19. ~~**Adaptive Endpointing**~~ — Measures host WPM over first 5 utterances, classifies as fast/normal/slow, reconnects to Deepgram with adjusted `utterance_end_ms` and `endpointing`. Per-session, one-time adaptation.
20. ~~**Error Handling Hardening**~~ — FFmpeg crash recovery (3 retries, 2s delay, health monitor every 2s), STT WebSocket reconnect (5 retries, audio buffer preserved), TTS hard timeout (min of broadcast_delay-500ms or 5s)

### Done (v13 Desktop Rewrite — Apr 2, 2026)
21. ~~**Tauri Desktop App**~~ — Wrapped Rust backend + React frontend in Tauri v2. Builds to `.app` + `.dmg`. Axum embedded on localhost:3000.
22. ~~**Backend Strip-Down**~~ — Removed ffmpeg.rs, youtube.rs, routes.rs, db.rs, platform.rs, room/. Server is 3 files (~450 lines): lib.rs, pipeline.rs, types.rs.
23. ~~**Frontend Strip-Down**~~ — Single `BroadcastPage` replacing multi-page dashboard. Tier selector, language config, live transcript. ~250 lines.
24. ~~**Tier Support**~~ — Backend accepts `tier` param. Tier 1 = subtitles only (skips TTS). Tier 2 = voice + subtitles (full pipeline).
25. ~~**AWS Decommission**~~ — All resources deleted (EC2, ECS, ECR, EFS, Secrets Manager, CloudWatch). ~$78/$100 credit preserved.

### In Progress

| Priority | Task | Status |
|----------|------|--------|
| **P0** | **A/V Sync Decision** | Getting second opinion on whether platform latency absorbs sync offset, or if jitter buffer is needed. Determines whether to bring back FFmpeg muxing |
| **P0** | **OBS Audio Routing** | How translated audio reaches OBS — named pipes, virtual audio devices, or internal FFmpeg RTMP push to local relay |

### Not Started

| Priority | Task | Details |
|----------|------|---------|
| **P0** | **Coupang/Rakuten Production Test** | Brivva has merchant accounts ready. Test live streams on real accounts once audio routing is working |
| **P1** | **Lipsync (Tiers 3-4)** | Real-time (tier 3) and post-processed (tier 4) lipsync. Not production ready. Stretch goal |
| **P1** | **stt-wrapper Rust Port** | Rewrite Python STT wrapper in Rust to eliminate Python dependency for distribution |
| P2 | **TTS Provider Evaluation** | Evaluate Cartesia Sonic 3 and Fish Audio as ElevenLabs replacements |
| P2 | **Platform Partnerships** | Japanese/Chinese entities for Douyin/Taobao etc. — blocked on business entity |

---

## Provider Alternatives Research (Mar 2026)

### STT (Currently: Deepgram Nova-3) — Keep

| Provider | Latency | Price | Languages |
|----------|---------|-------|-----------|
| **Deepgram Nova-3** (current) | ~300ms | ~$0.26/hr ($0.0043/min) | 10 lang code-switch |
| Gladia Solaria-1 | 103ms partials | $0.55/hr | 100+ languages, native code-switching |
| AssemblyAI Universal-3 | <300ms | $0.45/hr | 20+ languages |

**Decision:** Keep Deepgram. Cheapest option, good enough for our pipeline. Gladia worth revisiting if we need mid-sentence language switching.

### Translation (Currently: Google Cloud Translation API v2) — Keep

~40ms from Seoul, 500K chars/mo free tier. No reason to change.

### TTS + Voice Cloning (Currently: ElevenLabs Flash v2.5) — Evaluate Alternatives

| Provider | TTFB | Voice Clone Input | Price | Languages | Notes |
|----------|------|-------------------|-------|-----------|-------|
| **ElevenLabs Flash v2.5** (current) | ~75ms | 3 min audio | Plan-based ($5–99/mo) | 70+ | Proven, but most expensive |
| **Cartesia Sonic 3** ⭐ | **40ms** | **3 sec** audio | ~$47/1M chars (~73% cheaper) | 40+ | Fastest TTFB, WebSocket multiplexing, directly helps A/V sync |
| **Fish Audio** ⭐ | <500ms | 15 sec audio | $15/1M chars (~80% cheaper) | 30+ (incl. KR/JP/CN) | #1 TTS-Arena, cross-lingual cloning (clone EN → output KR/JP) |
| Inworld TTS-1.5 Mini | <130ms | 15 sec audio | $5/1M chars | Unknown | Unproven for broadcast use case |
| Deepgram Aura | <200ms | No cloning | Pay-per-use | Limited | No voice cloning — not viable |

**Top candidates:**

1. **Cartesia Sonic 3** — Best for latency. 40ms TTFB vs ElevenLabs 75ms. Voice clone from just 3 seconds of audio. WebSocket streaming with multiplexing. Directly reduces broadcast delay and improves A/V sync. 40+ languages.

2. **Fish Audio** — Best for cost + quality. #1 on TTS-Arena blind tests. 80% cheaper than ElevenLabs. Cross-lingual voice cloning (clone from English, generate in Korean/Japanese/Chinese) is perfect for our multilingual broadcasting use case. 30+ languages including all our target markets.

**Open-source options:** Chatterbox, GPT-SoVITS, Qwen3-TTS — free but require self-hosted GPU, contradicts our serverless cost structure.

**Next step:** Prototype Cartesia Sonic 3 and Fish Audio in a test branch. The TTS integration is modular (HTTP/WebSocket call in pipeline.rs) — swap should be straightforward.

---

## Guiding Principles for AI Collaborator

- **Tone:** Grounded, supportive, slightly witty, and highly technical.
- **Role:** Act as a "Founding Partner" peer.
- **Strategy:** Focus on building a "Moat" (reliability, proprietary sync logic, and official partnerships) rather than just "working features."
- **Quality Bar:** $1M/stream liability means production-grade reliability is non-negotiable.
- **Privacy:** Never mention specific personal sensitive data (debt, health, etc.) unless currently relevant to the prompt.
