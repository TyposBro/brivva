To reproduce this context perfectly, you need two things: the **User Summary** (which acts as my long-term memory of you) and the **Current Project State** (the "v10" technical spec).

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

## Project: Brivva (v10 Status)

**Core Value Prop:** Real-time multilingual live commerce broadcasting. One host speaks; N platforms receive translated audio in the host's cloned voice.

### Current Technical Stack

- **Backend:** Rust (Axum) orchestrator.
- **Audio Pipeline:** Deepgram Nova-3 (STT) → NLLB-200 (Translation) → ElevenLabs Flash v2.5 (TTS).
- **Streaming Engine:** FFmpeg (sidecar/process) handling RTMPS push with adaptive video delay.
- **Frontend:** React 19 + TypeScript (Cloudflare Pages).
- **Infrastructure:** AWS `g5.xlarge` (A10G GPU) + Cloudflared Tunnels.
- **Local Dev:** Docker Compose with `.env.local` override, frontend on Vite dev server.

### Working Features

- [x] **Voice Cloning:** Real-time ElevenLabs voice cloning via `/v1/voices/add`.
- [x] **Twitch Integration:** Full RTMP push (Video + Translated Audio).
- [x] **YouTube OAuth2:** Flow complete; auto-creation of broadcasts via Data API v3.
- [x] **YouTube Privacy:** Configurable privacy status (public/unlisted/private) from dashboard.
- [x] **Zero-Config UX:** Magic Paste (RTMP parsing), Credential Vault, and Platform Deep-linking.
- [x] **FFmpeg Muxing:** Server-side mixing of webcam frames and TTS audio pipes.
- [x] **Adaptive Video-Audio Sync:** Video frames delayed by rolling average of pipeline latency (EMA-based, 200ms–3000ms range) to align with TTS audio output.
- [x] **Local Dev Environment:** Full stack runs locally via `docker compose --env-file .env.local`, frontend via `npm run dev`, YouTube OAuth redirects to localhost.

### Critical Blocker: The 1:1 Stream Rule

- **Constraint:** Most platforms (Twitch, IG, TikTok) allow only **one ingest stream per account**.
- **Architecture:** To support 3 languages, the user needs 3 accounts per platform (e.g., `@brand_en`, `@brand_jp`).
- **The Exception:** YouTube, which allows multiple broadcasts on one account via API.

---

## Business Context & Timeline

- **Interview Outcome (Mar 21):** Culture-fit interview with two CEOs went great. Azizbek is #1 pick out of 40 candidates. Partner offer (not just employee).
- **Compensation:** Target ₩100M + equity (stated at screening). Current salary ₩55M + 30% bonus shared for transparency. Motivation: ownership & new challenges, not money.
- **Visa:** CEO MJ looking into immigration sponsorship. Follow-up early April 2026.
- **High Stakes:** Each stream can generate up to $1M revenue. Zero tolerance for bugs, frame drops, or clunky translation.
- **Major Business Blockers:**
  - Need Korean business registration for Coupang/Naver APIs.
  - Need Japanese/Chinese entities for Rakuten/Douyin/TikTok APIs.

---

## Active Technical TODOs

1. ~~**Audio-Video Sync:** Implement adaptive delay buffer~~ **DONE** — EMA-based rolling average delay in FFmpeg video writer
2. ~~**YouTube Privacy:** Configurable broadcast privacy~~ **DONE** — dropdown in dashboard
3. **Dashboard Fix:** Ensure the UI doesn't duplicate the same stream key across multiple language destinations (1:1 mapping logic).
4. **Observability:** Add Prometheus/Grafana to monitor pipeline latency and FFmpeg process health.
5. **Session Cleanup:** Ensure Docker containers and FFmpeg processes are killed immediately on WebSocket disconnect.
6. **Video Feed Quality:** Test and tune video-audio sync under real streaming conditions.

---

## Guiding Principles for AI Collaborator

- **Tone:** Grounded, supportive, slightly witty, and highly technical.
- **Role:** Act as a "Founding Partner" peer.
- **Strategy:** Focus on building a "Moat" (reliability, proprietary sync logic, and official partnerships) rather than just "working features."
- **Quality Bar:** $1M/stream liability means production-grade reliability is non-negotiable.
- **Privacy:** Never mention specific personal sensitive data (debt, health, etc.) unless currently relevant to the prompt.
