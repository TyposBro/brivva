# Brivva Project Context (Apr 2, 2026)

## Project: Brivva (v14 — Desktop App, Apr 2 2026)

**Core Value Prop:** Real-time multilingual live commerce broadcasting. One host speaks; N platforms receive translated audio in the host's cloned voice. Source-language platforms get the host's actual voice (passthrough). Single desktop app — no OBS, no Python, no cloud infrastructure.

### Technical Stack

- **Desktop App:** Tauri v2 (Rust + React frontend). Single binary, system tray support. Builds to `.app` + `.dmg` (macOS). FFmpeg bundled as sidecar.
- **Backend:** Rust (Axum) embedded in Tauri on localhost:3000. 5 files (~1800 lines): `lib.rs` (WebSocket server, tagged binary routing), `pipeline.rs` (STT→Translate→TTS→RTMP), `stt.rs` (Deepgram client, chunking, prosody, emotion), `ffmpeg.rs` (RTMP muxer, jitter buffer, crash recovery), `types.rs` (Lang, Session, ServerMsg).
- **Audio Pipeline:** Deepgram Nova-3 (STT, direct WebSocket, no Python wrapper) → Google Cloud Translation API v2 (~40ms) → ElevenLabs Flash v2.5 / Multilingual v2 (TTS, with voice cloning). Translated audio decoded MP3→PCM, truncated+fadeout, queued to RTMP jitter buffer.
- **Video Pipeline:** Camera → MediaRecorder (hardware VP8/H.264 encoder, any fps) → encoded chunks via tagged binary WebSocket (0x02) → delayed buffer (broadcast delay D) → FFmpeg stdin → re-encode libx264 ultrafast → FLV → RTMP. Source-language passthrough queues host audio directly (zero TTS cost).
- **Frontend:** React 19 + TypeScript + Tailwind 3. Single `BroadcastPage` (~500 lines) — tier selector, language config, RTMP destinations, settings (broadcast delay slider, camera/mic pickers), webcam preview, voice cloning card (30s recording + progress), live transcript, dismissible error banners.
- **STT:** Direct Deepgram Nova-3 WebSocket in Rust. Features: clause-boundary chunking for EN/JA/KO/ZH (marker-based detection), prosody extraction via autocorrelation (pitch, energy, pause density), emotion classification (excited/happy/angry/sad/serious/neutral), style param mapping to ElevenLabs voice_settings, adaptive endpointing (reconnects with tuned params after 5 utterances).
- **Protocol:** Tagged binary WebSocket — 0x01 = audio PCM (STT), 0x02 = video chunk (RTMP). JSON control messages: `rtmp:config`, `video:codec`, `voice:sample`, `voice:ready`, `error`.
- **Infrastructure:** None. All processing local. API keys from `.env.local`. FFmpeg bundled.
- **Translation Tiers:** tier 1 = subtitles only (STT + Translate, no TTS), tier 2 = voice + subtitles (full pipeline + RTMP). Tiers 3-4 (lipsync) not yet implemented.
- **Persistence:** All config (langs, tier, RTMP URLs, broadcast delay, device IDs) saved to localStorage.
- **System Tray:** Close window → minimize to tray. Streaming continues in background. Tray menu: Show/Quit.

### Estimated Cost Per Session (Desktop App — No Cloud Compute)

Per-stream API cost: ~$1.24 (2-hour session: Deepgram $0.52 + Google Translate $0.72).
Compute cost: $0 — runs locally on desktop. No AWS infrastructure.

| Service          | Pricing                 | 5 sessions/mo | 30 sessions/mo | 100 sessions/mo |
| ---------------- | ----------------------- | ------------- | -------------- | --------------- |
| Deepgram Nova-3  | $0.0043/min             | $2.60         | $15.60         | $52             |
| Google Translate | $20/M chars (500K free) | $0            | $7.80          | $52             |
| ElevenLabs TTS   | Plan-based              | $5            | $22            | $99             |
| **Total**        |                         | **~$8**       | **~$45**       | **~$203**       |

### Key Design Decisions & Lessons Learned

1. **No H.264 passthrough:** `-c:v copy` crashes FFmpeg when reading chunked WebM from MediaRecorder via stdin (SIGPIPE/SIGABRT). Always re-encode with `libx264 ultrafast`. CPU cost minimal since input already compressed.
2. **MediaRecorder over canvas JPEG:** Old approach (canvas→JPEG→base64→JSON per frame) bottlenecked at 15fps. MediaRecorder uses hardware encoder, handles any fps, sends pre-compressed chunks.
3. **Direct Deepgram over Python wrapper:** Eliminated Python dependency. All STT logic (chunking, prosody, emotion) ported to Rust. Single binary.
4. **Tagged binary protocol:** 0x01/0x02 byte prefix distinguishes audio/video in the same WebSocket. Clean separation without JSON overhead for high-frequency data.
5. **Adaptive endpointing:** After 5 utterances, classifies speaker speed (fast/normal/slow) and reconnects to Deepgram with tuned endpointing/utterance_end_ms. Improves transcription quality for different speaking styles.

### Working Features

- [x] Tauri desktop app (macOS `.app` + `.dmg`, system tray)
- [x] WebSocket server on localhost:3000 (tagged binary routing)
- [x] Microphone capture (44.1kHz PCM, device selection)
- [x] Webcam capture (MediaRecorder, hardware encoded, any fps, device selection)
- [x] STT (Deepgram Nova-3, direct WebSocket, clause chunking, adaptive endpointing)
- [x] Translation (Google Cloud API v2, ~40ms, parallel per language)
- [x] TTS (ElevenLabs Flash v2.5 + Multilingual v2 for cloned voices)
- [x] Voice cloning (30s recording UI, progress bar, auto-activate)
- [x] FFmpeg RTMP muxing (video chunk drain + audio jitter buffer, crash recovery)
- [x] Source-language passthrough (host audio → RTMP, zero TTS cost)
- [x] RTMP destination config (per-language URL inputs)
- [x] Broadcast delay slider (1-10s, configurable)
- [x] Settings persistence (localStorage)
- [x] Error banners (WS disconnect, RTMP failures, backend errors)
- [x] FFmpeg bundled as Tauri sidecar (no system install needed)
- [x] Prosody extraction + emotion classification + style param mapping
- [x] System tray (minimize to tray, streaming continues)

### Not Yet Implemented

| Priority | Task                        | Details                                                       |
| -------- | --------------------------- | ------------------------------------------------------------- |
| **P0**   | **Production testing**      | Test on Coupang, Rakuten, YouTube with real merchant accounts |
| **P1**   | **Lipsync (Tiers 3-4)**     | Real-time and post-processed lipsync. Stretch goal            |
| **P1**   | **TTS Provider Evaluation** | Cartesia Sonic 3 (40ms TTFB) and Fish Audio (80% cheaper)     |
| P2       | **macOS code signing**      | Need Apple Developer account ($99/year)                       |
| P2       | **Platform partnerships**   | Japanese/Chinese entities for Douyin/TikTok                   |

---

## Business Context & Timeline

- **Partnership:** Pivoted from employment to profit-sharing/CTO arrangement. CEOs disagree: MJ wants salary (no profit share), Simon wants profit split. Decision deferred until Sep 2026.
- **Revenue model:** ₩100M per contract (total, before client cut). Client takes a cut (varies per contract), then costs (~₩18M for influencer/crew/studio), then profit split. Aziz's 30% depends on client cut — ₩9.6M-₩15.6M per contract.
- **Competitive landscape:** Prism (Naver-owned) is direct competitor. Brivva's desktop app approach bypasses Naver dependency.
- **Lipsync roadmap:** Simon says tech not ready for 6mo-1yr. Plan: ship Options 1 & 2 now, A/B test all 4 tiers when lipsync matures. Simon built a crappy lipsync demo with Claude Code — tech is accessible but quality isn't there.
- **Demo deadline:** In-person demo with Simon + MJ being scheduled for a weekend. Production target April 26.
- **High stakes:** Each stream can generate up to $1M revenue. Zero tolerance for bugs or frame drops.
- **Current status (Apr 4):** Desktop app fully built (v14). Simon checking with MJ on weekend demo time. Next: in-person demo with both CEOs.
- **Simon's hands-on test (Apr 2):** Tested product live at Starbucks — spoke Japanese to camera as live commerce host, recorded video. Main complaint: TTS lag/freezing. Noted sentence chunking issue (2s vs 10s utterances need full sentence for translation). Production costs $2-3K per live show. Despite later saying "we don't need the tech" — he was already using it. Pushback was negotiation posturing.
- **Priority for MJ demo:** Reduce end-to-end translation latency. ElevenLabs Flash v2.5 is staying (Cartesia Sonic 3 tested — low quality, low speed). The real bottleneck may be sentence chunking (STT waits 2-10s for full sentence before translating), not TTS speed. Fix: tune adaptive endpointing / utterance segmentation for shorter chunks.
- **Revenue reality:** ₩6M per contract to Aziz, 10 contracts/year = ₩60M (below ₩90M salary posting). Salary route (MJ's preference) is financially better until volume scales. Don't quit StoneLab until 3+ months proven revenue.
- **Long-term signal:** Simon said he wants to work with Aziz on other projects too, even if Brivva doesn't work out.

---

## Guiding Principles for AI Collaborator

- **Tone:** Grounded, supportive, slightly witty, and highly technical.
- **Role:** Act as a "Founding Partner" peer.
- **Strategy:** Focus on reliability and shipping. The app IS the streaming engine — no OBS dependency.
- **Quality Bar:** $1M/stream liability means production-grade reliability is non-negotiable.
