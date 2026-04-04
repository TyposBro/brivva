# Brivva Project Context (Apr 4, 2026)

## Project: Brivva (v15 — Progressive Chunking, Apr 4 2026)

**Core Value Prop:** Real-time multilingual live commerce broadcasting. One host speaks; N platforms receive translated audio in the host's cloned voice. Source-language platforms get the host's actual voice (passthrough). Single desktop app — no OBS, no Python, no cloud infrastructure.

### Technical Stack

- **Desktop App:** Tauri v2 (Rust + React frontend). Single binary, system tray support. Builds to `.app` + `.dmg` (macOS). FFmpeg bundled as sidecar.
- **Backend:** Rust (Axum) embedded in Tauri on localhost:3000. 5 files (~3100 lines): `lib.rs` (WebSocket server, tagged binary routing), `pipeline.rs` (STT→Translate→TTS→RTMP with progressive chunking), `stt.rs` (Gladia client, progressive chunk detection, prosody, emotion), `ffmpeg.rs` (RTMP muxer, jitter buffer, incremental MP3 decoder, crash recovery), `types.rs` (Lang, Session, ChunkEvent, ServerMsg).
- **Audio Pipeline:** Gladia Solaria-1 (STT, direct WebSocket) → ProgressiveChunkDetector (clause-boundary splitting during interims) → Google Cloud Translation API v2 (~130-376ms, context-aware) → ElevenLabs Flash v2.5 (TTS, WebSocket streaming with IncrementalMp3Decoder) → StreamingPcm accumulator → FFmpeg audio FIFO → AAC → RTMP.
- **Video Pipeline:** Camera → MediaRecorder (hardware VP8/H.264 encoder, any fps) → encoded chunks via tagged binary WebSocket (0x02) → delayed buffer (D seconds) → FFmpeg stdin → re-encode libx264 ultrafast → FLV → RTMP. Source-language passthrough queues host audio directly (zero TTS cost).
- **Frontend:** React 19 + TypeScript + Tailwind 3. Single `BroadcastPage` (~500 lines) — tier selector, language config, RTMP destinations, settings (broadcast delay slider, camera/mic pickers), webcam preview, voice cloning card, live transcript with progressive chunk translations, dismissible error banners.
- **STT:** Gladia Solaria-1 WebSocket in Rust (switched from Deepgram Nova-3). Features: ProgressiveChunkDetector for clause-boundary splitting during interims (language-adaptive: EN 1000ms/2000ms, JA/KO 800ms/2000ms, ZH 800ms/2000ms), normalized position tracking resilient to Gladia transcript revisions (derive_position), prosody extraction via autocorrelation, emotion classification, style param mapping, adaptive endpointing.
- **TTS:** ElevenLabs Flash v2.5 via WebSocket streaming. IncrementalMp3Decoder decodes MP3 chunks to PCM in real-time (not batch). Each decoded chunk appended to StreamingPcm immediately — audio drain starts receiving PCM within ~75ms TTFB instead of waiting for full generation.
- **Protocol:** Tagged binary WebSocket — 0x01 = audio PCM (STT), 0x02 = video chunk (RTMP). JSON control messages: `rtmp:config`, `video:codec`, `voice:sample`, `voice:ready`, `error`.
- **Infrastructure:** None. All processing local. API keys from `.env.local`. FFmpeg bundled.
- **Translation Tiers:** tier 1 = subtitles only (STT + Translate, no TTS), tier 2 = voice + subtitles (full pipeline + RTMP). Tiers 3-4 (lipsync) not yet implemented.
- **Persistence:** All config (langs, tier, RTMP URLs, broadcast delay, device IDs) saved to localStorage.
- **System Tray:** Close window → minimize to tray. Streaming continues in background. Tray menu: Show/Quit.

### v14 → v15 Changes (Apr 4, 2026)

1. **IncrementalMp3Decoder** (`ffmpeg.rs`) — Long-lived FFmpeg subprocess for streaming MP3→PCM decode. Pre-drains stdout before writing (prevents pipe deadlock). Graceful finish with 500ms drain timeout.
2. **ProgressiveChunkDetector** (`stt.rs`) — Replaces single-shot ChunkDetector. Emits sub-utterance chunks at clause boundaries during interims. Tracks emitted text (not byte positions) for resilience to Gladia transcript revisions. Language-specific clause markers with tuned thresholds.
3. **Progressive pipeline** (`pipeline.rs`) — `spawn_chunked_pipeline()` creates one StreamingPcm per (utterance, language) and feeds it from multiple sequential TTS chunks. Context-aware translation (previous chunk prepended with `|||` separator). Backpressure: skips chunks if pipeline exceeds broadcast delay.
4. **Default broadcast delay** reduced 5000ms → 3000ms.
5. **Frontend** handles `chunk_translation` messages with progressive append display.

### Estimated Cost Per Session (Desktop App — No Cloud Compute)

Per-stream API cost: ~$1.24 (2-hour session: Gladia $0.52 + Google Translate $0.72).
Compute cost: $0 — runs locally on desktop. No AWS infrastructure.

| Service          | Pricing                 | 5 sessions/mo | 30 sessions/mo | 100 sessions/mo |
| ---------------- | ----------------------- | ------------- | -------------- | --------------- |
| Gladia Solaria-1 | ~$0.0043/min            | $2.60         | $15.60         | $52             |
| Google Translate | $20/M chars (500K free) | $0            | $7.80          | $52             |
| ElevenLabs TTS   | Plan-based              | $5            | $22            | $99             |
| **Total**        |                         | **~$8**       | **~$45**       | **~$203**       |

### Key Design Decisions & Lessons Learned

1. **No H.264 passthrough:** `-c:v copy` crashes FFmpeg when reading chunked WebM from MediaRecorder via stdin (SIGPIPE/SIGABRT). Always re-encode with `libx264 ultrafast`. CPU cost minimal since input already compressed.
2. **MediaRecorder over canvas JPEG:** Old approach (canvas→JPEG→base64→JSON per frame) bottlenecked at 15fps. MediaRecorder uses hardware encoder, handles any fps, sends pre-compressed chunks.
3. **Direct STT WebSocket in Rust:** Eliminated Python dependency. All STT logic (chunking, prosody, emotion) in Rust. Single binary.
4. **Tagged binary protocol:** 0x01/0x02 byte prefix distinguishes audio/video in the same WebSocket. Clean separation without JSON overhead for high-frequency data.
5. **Incremental MP3 decode:** Batch decode (accumulate all chunks → single FFmpeg call) added 500-1500ms latency. Incremental decode (long-lived FFmpeg subprocess, pre-drain stdout) streams PCM to RTMP within TTFB.
6. **Progressive chunking over full-sentence translation:** Splitting at clause boundaries during interims reduces perceived latency by ~7s for long utterances. Context-aware translation (`|||` separator) mitigates quality loss.
7. **Transcript instability handling:** Gladia revises interim text (adds punctuation, corrects words). `derive_position` uses normalized character matching instead of byte offsets.
8. **Pipeline is commodity, not moat:** Emerging APIs (Alibaba Qwen3-LiveTranslate, Palabra AI, Soniox semantic VAD) can replace STT+Translate+TTS. Brivva's value is the broadcasting experience — RTMP muxing, A/V sync, voice clone persistence, multi-platform seller UX.

### Working Features

- [x] Tauri desktop app (macOS `.app` + `.dmg`, system tray)
- [x] WebSocket server on localhost:3000 (tagged binary routing)
- [x] Microphone capture (44.1kHz PCM, device selection)
- [x] Webcam capture (MediaRecorder, hardware encoded, any fps, device selection)
- [x] STT (Gladia Solaria-1, direct WebSocket, progressive chunk detection, adaptive endpointing)
- [x] Translation (Google Cloud API v2, context-aware with `|||` separator, parallel per language)
- [x] TTS (ElevenLabs Flash v2.5, incremental MP3→PCM streaming via IncrementalMp3Decoder)
- [x] Progressive chunking (ProgressiveChunkDetector, language-adaptive thresholds, Gladia revision resilience)
- [x] Voice cloning (30s recording UI, progress bar, auto-activate, persisted to disk)
- [x] FFmpeg RTMP muxing (video chunk drain + audio jitter buffer, crash recovery)
- [x] Source-language passthrough (host audio → RTMP, zero TTS cost)
- [x] RTMP destination config (per-language URL inputs)
- [x] Broadcast delay slider (1-10s, default 3s)
- [x] Settings persistence (localStorage)
- [x] Error banners (WS disconnect, RTMP failures, backend errors)
- [x] FFmpeg bundled as Tauri sidecar (no system install needed)
- [x] Prosody extraction + emotion classification + style param mapping
- [x] System tray (minimize to tray, streaming continues)
- [x] Progressive chunk translation display in frontend

### Not Yet Implemented

| Priority | Task                        | Details                                                       |
| -------- | --------------------------- | ------------------------------------------------------------- |
| **P0**   | **Evaluate end-to-end APIs** | Palabra AI, Alibaba Qwen3-LiveTranslate, Pinch — may replace Gladia+Google+ElevenLabs |
| **P0**   | **Production testing**      | Test on Coupang, Rakuten, YouTube with real merchant accounts |
| **P1**   | **Semantic STT upgrade**    | Soniox v4 or OpenAI semantic_vad as Gladia replacement        |
| **P1**   | **Lipsync (Tiers 3-4)**     | Real-time and post-processed lipsync. Stretch goal            |
| P2       | **macOS code signing**      | Need Apple Developer account ($99/year)                       |
| P2       | **Platform partnerships**   | Japanese/Chinese entities for Douyin/TikTok                   |

---

## Business Context & Timeline

- **Partnership:** Pivoted from employment to profit-sharing/CTO arrangement. CEOs disagree: MJ wants salary (no profit share), Simon wants profit split. Decision deferred until Sep 2026.
- **Revenue model:** ₩100M per contract (total, before client cut). Client takes a cut (varies per contract), then costs (~₩18M for influencer/crew/studio), then profit split. Aziz's 30% depends on client cut — ₩9.6M-₩15.6M per contract.
- **Competitive landscape:** Prism (Naver-owned) is direct competitor. Brivva's desktop app approach bypasses Naver dependency. Emerging end-to-end APIs (Palabra, Qwen3-LiveTranslate) may commoditize the translation pipeline — Brivva's moat is the broadcasting experience, not the AI.
- **Lipsync roadmap:** Simon says tech not ready for 6mo-1yr. Plan: ship Options 1 & 2 now, A/B test all 4 tiers when lipsync matures.
- **Demo deadline:** In-person demo with Simon + MJ being scheduled for a weekend. Production target April 26.
- **High stakes:** Each stream can generate up to $1M revenue. Zero tolerance for bugs or frame drops.
- **Current status (Apr 4):** Desktop app v15 with progressive chunking. Default delay reduced to 3s. Incremental TTS decode saves 500-1500ms. Strategic question: build vs buy the translation pipeline.
- **Simon's hands-on test (Apr 2):** Tested product live at Starbucks — spoke Japanese to camera as live commerce host. Main complaint: TTS lag/freezing. Noted sentence chunking issue. Progressive chunking addresses this directly.
- **Priority for MJ demo:** Demonstrate reduced latency with progressive chunking. Or: demo with Palabra/Qwen3 if evaluation shows better results.
- **Revenue reality:** ₩6M per contract to Aziz, 10 contracts/year = ₩60M (below ₩90M salary posting). Salary route (MJ's preference) is financially better until volume scales. Don't quit StoneLab until 3+ months proven revenue.
- **Long-term signal:** Simon said he wants to work with Aziz on other projects too, even if Brivva doesn't work out.

---

## Guiding Principles for AI Collaborator

- **Tone:** Grounded, supportive, slightly witty, and highly technical.
- **Role:** Act as a "Founding Partner" peer.
- **Strategy:** Focus on reliability and shipping. The app IS the streaming engine — no OBS dependency. The translation pipeline is a commodity — the moat is the broadcasting experience.
- **Quality Bar:** $1M/stream liability means production-grade reliability is non-negotiable.
