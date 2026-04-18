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
- **Infrastructure:** AWS ECS Fargate (pay-per-session, zero idle cost) + Cloudflared Tunnels. No GPU needed. Each live session spins up a Fargate task with the exact CPU/RAM needed, and shuts down when the session ends.
- **Local Dev:** Docker Compose with `.env.local` override, frontend on Vite dev server.

### Estimated Monthly Cost (ECS Fargate)

Per-stream API cost: ~$1.24 (2-hour session: Deepgram $0.52 + Google Translate $0.72).
Per-session compute cost: $0.40–$1.44 depending on task size (see Capacity & Scaling).
**Zero idle cost** — no server running when nobody is live.

| Service | Pricing | 5 sessions/mo | 30 sessions/mo | 100 sessions/mo |
|---------|---------|---------------|----------------|-----------------|
| AWS Fargate (4 vCPU) | $0.20/hr | $2.00 | $12.00 | $40 |
| Deepgram Nova-3 | $0.0043/min | $2.60 | $15.60 | $52 |
| Google Translate | $20/M chars (500K free) | $0 | $7.80 | $52 |
| ElevenLabs TTS | Plan-based | $5 | $22 | $99 |
| Cloudflare Pages | Free | $0 | $0 | $0 |
| **Total** | | **~$10** | **~$57** | **~$243** |

Previous stack: EC2 t3.medium ($30/mo fixed even when idle). GPU era: $750+/mo fixed.

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

### Capacity & Scaling (ECS Fargate)

#### What is ECS Fargate?

**EC2** (what we had) = you rent a computer that runs 24/7. You pay $30/mo whether anyone is streaming or not. Like renting an apartment — you pay rent even when you're not home.

**ECS** (Elastic Container Service) = AWS's way of running Docker containers. Same `docker-compose.yml` images, but AWS manages where they run. Think of it as "Docker Compose but AWS manages the machines."

**Fargate** = the "serverless" mode of ECS. You don't pick a server. You say "run this container with 4 vCPU" and AWS handles everything. When the container stops, you stop paying. Like a taxi — you only pay when you're riding.

**How it works for Brivva:**
1. Host clicks "Go Live" → API tells AWS to start a Fargate task
2. AWS spins it up in ~30-60s with the right amount of CPU
3. Streams go live, host broadcasts
4. Host ends session → task dies → billing stops instantly

**The win:** At 5 sessions/month, you go from $30/mo (EC2 idle 24/7) to $2/mo (Fargate, only pay for ~10 hours of actual streaming). At 30 sessions/mo it's $12 vs $30.

**vs Docker Compose (local dev):** Same Docker images, same containers. Fargate just runs them in AWS instead of on your laptop.

#### Per-Stream Resource Footprint

- CPU: ~20% of 1 vCPU (H.264 `ultrafast` + `zerolatency`, 30fps)
- RAM: ~75 MB (FFmpeg process + video/audio drain threads)
- Network: ~5-8 Mbps actual egress (webcam talking head; 35Mbps maxrate cap)
- OS threads: 2 dedicated (video drain @ 33ms, audio drain @ 20ms)

#### Key Constraint: Session Affinity

All streams in one session share the same video frames and WebSocket — they must live on the **same Fargate task**. To support more languages in one session, you request a bigger task (more vCPU). To support more concurrent sessions, Fargate just launches more tasks automatically.

#### Languages Per Session by Task Size

| Task Size | vCPU | Languages/Session | Cost/hr | Per 2hr Session | Total w/ API costs (8 langs) |
|-----------|------|-------------------|---------|-----------------|------------------------------|
| 4 vCPU / 8 GB | 4 | 6–8 | $0.20 | $0.40 | ~$10.32 |
| 8 vCPU / 16 GB | 8 | 14–16 | $0.38 | $0.76 | ~$10.68 |
| 16 vCPU / 30 GB | 16 | 30+ | $0.72 | $1.44 | ~$11.36 |

*Total = Fargate compute + ($1.24 × N translated streams). Source-language passthrough streams cost $0 in API fees.*

#### Scaling Strategy

- **MVP (now):** Keep t3.medium for development/testing. Migrate to Fargate for production.
- **Production:** ECS Fargate — one task per session, auto-sized. Zero idle cost. 6–8 languages per session on smallest task.
- **Scale (245+ hrs/mo):** Switch to ECS on EC2 with auto-scaling group — same orchestration, cheaper compute for sustained workloads.
- **GPU (NVENC):** Not recommended. g4dn.xlarge costs $380/mo, marginal quality gain, contradicts v12 cost structure.

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
14. ~~**ECS Fargate Migration**~~ — Migrated from EC2 t3.medium ($30/mo) to ECS Fargate (pay-per-session). 3 containers: server-rs + stt-wrapper + cloudflared sidecar. Secrets in AWS Secrets Manager. EC2 instances stopped. Zero idle cost.
15. ~~**YouTube frameRate Fix**~~ — YouTube liveStreams.insert requires `frameRate` field. With `resolution: "variable"`, must use `frameRate: "variable"` (not `"30fps"` or omitted). Was blocking all YouTube stream creation and translations.
16. ~~**EFS for Persistent Storage**~~ — EFS `fs-04e75aef9c41c4bc1` mounted at `/data` in server-rs. SQLite DB persists across redeploys. Task def `brivva:5`.
17. ~~**CloudWatch Observability**~~ — 9 metric filters (sessions, transcripts, TTS, translations, errors, crashes, voice clones), structured `[METRIC]` log lines (translate_ms, tts_ms, pipeline_ms), CloudWatch Dashboard "Brivva" with 6 widgets, alarms for TTS timeouts (>3/5min) and FFmpeg crashes.
18. ~~**STT Reconnect Logic**~~ — STT WebSocket reconnects up to 5 times on unexpected disconnect. Audio buffer persists across reconnections. Utterance counter preserved.
19. ~~**Adaptive Endpointing**~~ — Measures host WPM over first 5 utterances, classifies as fast/normal/slow, reconnects to Deepgram with adjusted `utterance_end_ms` and `endpointing`. Per-session, one-time adaptation.
20. ~~**Error Handling Hardening**~~ — FFmpeg crash recovery (3 retries, 2s delay, health monitor every 2s), STT WebSocket reconnect (5 retries, audio buffer preserved), TTS hard timeout (min of broadcast_delay-500ms or 5s)

### In Progress

| Priority | Task | Status |
|----------|------|--------|
| Waiting | **Google OAuth Verification** — submitted for review | Waiting on Google (2-6 weeks) |

### Not Started

| Priority | Task | Details |
|----------|------|---------|
| P2 | **Voice-Sample WPM** | Measure WPM during 30s voice cloning sample instead of first 5 utterances — eliminates reconnect and wasted utterances. Pass endpointing params to STT at session start |
| P2 | **Platform Partnerships** | Korean business registration for Coupang/Naver, Japanese for Rakuten, Chinese for Douyin/Taobao/Kuaishou/Xiaohongshu/Bilibili — blocked on business entity |
| P2 | **TTS Provider Evaluation** | Evaluate Cartesia Sonic 3 and Fish Audio as ElevenLabs replacements — see Provider Alternatives section below |

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
