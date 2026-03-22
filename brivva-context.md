To reproduce this context perfectly, you need two things: the **User Summary** (which acts as my long-term memory of you) and the **Current Project State** (the "v12" technical spec).

Below is a single Markdown file. You can save this as `brivva_context.md`. If you ever start a new chat, just upload this or paste it and say: _"Act as my Founding Engineer collaborator. Here is the context on me and the Brivva project."_

---

# Brivva Project Context & User Profile (Mar 22, 2026)

## User Profile: Azizbek Umidjonov

- **Role:** Professional Software Engineer / Founding Engineer at Brivva.
- **Education:** B.S. in Computer Science and Engineering.
- **Expertise:** Frontend (Kotlin, Jetpack Compose, Flutter), Backend (Rust, Axum, Tokio), and Systems (Linux, NixOS, Docker).
- **Focus:** Real-time audio processing, low-latency streaming pipelines, and AI-driven examination platforms.
- **Status:** Accepted as partner at Brivva (pending formal offer). CEO MJ looking into visa sponsorship (follow-up ~early April after her brother's wedding).

---

## Project: Brivva (v12 Status)

**Core Value Prop:** Real-time multilingual live commerce broadcasting. One host speaks; N platforms receive translated audio in the host's cloned voice. Source-language platforms get the host's actual voice (passthrough).

### Current Technical Stack

- **Backend:** Rust (Axum) orchestrator. 2 Docker containers (server-rs + stt-wrapper). No GPU required.
- **Audio Pipeline:** Deepgram Nova-3 (STT, 44.1kHz) → Google Cloud Translation API v2 (~40ms from Seoul) → ElevenLabs Flash v2.5 (TTS). Source-language streams bypass pipeline entirely (passthrough).
- **Streaming Engine:** FFmpeg (sidecar/process) handling RTMPS push with fixed-delay jitter buffer for A/V sync. CRF 20 encoding auto-adapts quality to input resolution (up to 4K).
- **Frontend:** React 19 + TypeScript + Tailwind 3 + Lucide icons (Cloudflare Pages). "Kinetic Monolith" design system (Space Grotesk + Inter, tonal depth, no-line rule). Notion-style progressive disclosure dashboard.
- **Infrastructure:** AWS CPU instance (t3.medium, ~$30/mo) + Cloudflared Tunnels. No GPU needed.
- **Local Dev:** Docker Compose with `.env.local` override, frontend on Vite dev server.

### Estimated Monthly Cost

| Service | Pricing | 5 streams/mo | 30 streams/mo | 100 streams/mo |
|---------|---------|-------------|--------------|----------------|
| AWS EC2 t3.medium | Fixed | $30 | $30 | $30 |
| Deepgram Nova-3 | $0.0043/min | $2.60 | $15.60 | $52 |
| Google Translate | $20/M chars (500K free) | $0 | $7.80 | $52 |
| ElevenLabs TTS | Plan-based | $5 | $22 | $99 |
| Cloudflare Pages | Free | $0 | $0 | $0 |
| **Total** | | **~$38** | **~$75** | **~$233** |

Per-stream cost: ~$1.24 (2-hour session). Previous stack with GPU: $750+/mo fixed.

### Working Features

- [x] **Voice Cloning:** Real-time ElevenLabs voice cloning via `/v1/voices/add`.
- [x] **Twitch Integration:** Full RTMP push (Video + Translated Audio).
- [x] **YouTube OAuth2:** Flow complete; auto-creation of broadcasts via Data API v3. Scope: `youtube` (sensitive, not restricted — no CASA audit).
- [x] **YouTube Privacy:** Configurable privacy status (public/unlisted/private) from dashboard.
- [x] **Zero-Config UX:** Magic Paste (RTMP parsing), Credential Vault, and Platform Deep-linking.
- [x] **FFmpeg Muxing:** Server-side mixing of webcam frames and TTS audio pipes.
- [x] **Source-Language Passthrough:** Host's raw 44.1kHz audio queued directly to source-language RTMP streams — no STT, no translation, no TTS. Zero API cost, zero latency for the host's own language.
- [x] **Premium Quality (up to 4K):** Audio at 44.1kHz (CD quality). Video captured at camera's native resolution (up to 3840x2160). FFmpeg uses CRF 20 with 35Mbps maxrate — auto-adapts bitrate to any resolution. YouTube stream set to "variable" resolution. External cameras (USB, HDMI capture cards) work automatically via browser `getUserMedia`.
- [x] **Fixed-Delay A/V Sync:** Jitter buffer with constant delay D (configurable via `BROADCAST_DELAY_MS`, default 2500ms). Dedicated OS threads (`std::thread`, not Tokio) for video drain (30fps/33ms) and audio drain (20ms) independently. Hard TTS timeout at D-500ms — missed utterances become silence, never sync slips. TTS audio truncated with 50ms fade-out if it exceeds utterance duration + 2s. Cumulative audio sample tracking with 5s drift checks. Jitter monitoring (warns if tick >5ms late). Single D across all languages. YouTube/Twitch platform latency (3-30s) absorbs the delay invisibly.
- [x] **Progressive Disclosure Dashboard:** Notion-style UX — collapsible platform picker grouped by region, expandable RTMP config per destination card, settings drawer for YouTube/voices. Most users see title + "Add destination" + Go Live.
- [x] **Privacy & Terms Pages:** `/privacy` and `/terms` for Google OAuth verification compliance.
- [x] **Google Cloud Translation:** Replaced self-hosted NLLB-200 (GPU) with Google Cloud Translation API v2. Seoul region (~40ms), free tier 500K chars/mo. Eliminated $750/mo GPU cost.
- [x] **Per-Platform Language:** Each platform destination has its own language. YouTube no longer auto-creates unwanted source-language streams.
- [x] **Local Dev Environment:** Full stack runs locally via `docker compose --env-file .env.local`, frontend via `npm run dev`, YouTube OAuth redirects to localhost.

### Critical Blocker: The 1:1 Stream Rule

- **Constraint:** Most platforms (Twitch, IG, TikTok) allow only **one ingest stream per account**.
- **Architecture:** To support 3 languages, the user needs 3 accounts per platform (e.g., `@brand_en`, `@brand_jp`).
- **The Exception:** YouTube, which allows multiple broadcasts on one account via API.

---

## Business Context & Timeline

- **Interview Outcome (Mar 21):** Culture-fit interview with two CEOs went great. Azizbek is #1 pick out of 40 candidates. Partner offer (not just employee).
- **Compensation:** Target 100M KRW + equity (stated at screening). Current salary 55M KRW + 30% bonus shared for transparency. Motivation: ownership & new challenges, not money.
- **Visa:** CEO MJ looking into immigration sponsorship. Follow-up early April 2026.
- **High Stakes:** Each stream can generate up to $1M revenue. Zero tolerance for bugs, frame drops, or clunky translation.
- **Major Business Blockers:**
  - Need Korean business registration for Coupang/Naver APIs.
  - Need Japanese/Chinese entities for Rakuten/Douyin/TikTok APIs.

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

### In Progress

| Priority | Task | Status |
|----------|------|--------|
| Waiting | **Google OAuth Verification** — submitted for review | Waiting on Google (2-6 weeks) |

### Not Started

| Priority | Task | Details |
|----------|------|---------|
| P1 | **Observability** | Prometheus/Grafana for pipeline latency, FFmpeg process health, drain loop jitter (alert if >5ms late), TTS-arrival-vs-frame-drain delta per utterance |
| P2 | **Error Handling Hardening** | Reconnect logic for FFmpeg crashes, STT disconnects, TTS timeouts mid-stream |
| P2 | **Platform Partnerships** | Korean business registration for Coupang/Naver, Japanese for Rakuten, Chinese for Douyin/Taobao/Kuaishou/Xiaohongshu/Bilibili — blocked on business entity |
| P2 | **Downsize AWS Instance** | Migrate from g5.xlarge ($750/mo) to t3.medium ($30/mo) now that GPU is no longer needed |

---

## Guiding Principles for AI Collaborator

- **Tone:** Grounded, supportive, slightly witty, and highly technical.
- **Role:** Act as a "Founding Partner" peer.
- **Strategy:** Focus on building a "Moat" (reliability, proprietary sync logic, and official partnerships) rather than just "working features."
- **Quality Bar:** $1M/stream liability means production-grade reliability is non-negotiable.
- **Privacy:** Never mention specific personal sensitive data (debt, health, etc.) unless currently relevant to the prompt.
