# Brivva Desktop App — Implementation Plan

**Date:** April 4, 2026
**Target:** Working demo by April 30, 2026
**Constraint:** Weekend builds only (StoneLab weekdays)

---

## Architecture

**Fixed-delay jitter buffer** — one global delay D (default 3s) for all languages. App handles video + audio muxing internally via FFmpeg and pushes N RTMP streams directly to platforms. No OBS needed.

**Video pipeline:** Camera → MediaRecorder (hardware VP8/H.264) → encoded chunks → WebSocket (binary 0x02) → delayed buffer (D seconds) → FFmpeg stdin → re-encode libx264 ultrafast → FLV → RTMP

**Audio pipeline (v15 — progressive chunking):** Mic → PCM 44.1kHz → WebSocket (binary 0x01) → Gladia Solaria-1 STT → ProgressiveChunkDetector (clause-boundary splitting during interims) → Google Translate (context-aware, `|||` separator) → ElevenLabs Flash v2.5 TTS → IncrementalMp3Decoder (streaming MP3→PCM) → StreamingPcm accumulator → FFmpeg audio FIFO → AAC → RTMP

**Key design decisions:**
- `-c:v copy` (passthrough) doesn't work with chunked WebM from MediaRecorder stdin — FFmpeg crashes. Always re-encode with `ultrafast`. CPU cost is low since input is already compressed.
- Platform latency does NOT absorb sync gaps — offset encoded = offset viewed
- Detection threshold: ~125ms audio-late, ~185ms annoying
- Variability is worse than consistent delay — brain adapts to fixed offset in ~30s
- Studio cameras (Elgato/BlackMagic capture cards) present as USB devices to getUserMedia
- Translation pipeline is commodity — moat is broadcasting experience, not AI

---

## Current State (end of Apr 4 session)

### Single binary — no external dependencies

- **Tauri v2 desktop app** — builds to `.app` + `.dmg` (macOS), system tray support
- **Backend:** 5 Rust files (~3100 lines) — `lib.rs`, `pipeline.rs`, `ffmpeg.rs`, `stt.rs`, `types.rs`
- **Frontend:** Single-page `BroadcastPage` (~500 lines) — tier selector, language config, RTMP destinations, settings (delay slider, device pickers), webcam preview, voice cloning, live transcript with progressive chunk translations, error banners
- **FFmpeg:** Bundled as Tauri sidecar (77MB static binary, gitignored)
- **STT:** Gladia Solaria-1 WebSocket in Rust. ProgressiveChunkDetector for clause-boundary splitting, prosody analysis, emotion classification, adaptive endpointing.
- **TTS:** ElevenLabs Flash v2.5 WebSocket with IncrementalMp3Decoder (streaming MP3→PCM, not batch).
- **Video:** MediaRecorder with hardware encoder, any resolution/fps. Re-encoded via libx264 ultrafast for FLV/RTMP.
- **Protocol:** Tagged binary WebSocket (0x01=audio, 0x02=video) + JSON control messages (rtmp:config, video:codec, voice:sample)

---

## Implementation Phases

### Phase 1: Core Pipeline + Single Binary (DONE — Apr 2)

1. Restored `ffmpeg.rs` — jitter buffer, audio drain OS threads, RTMP push, crash recovery (50 retries), orphan cleanup
2. MediaRecorder video capture — hardware-encoded at any fps, tagged binary protocol (0x02)
3. Direct Deepgram STT in Rust — replaces Python stt-wrapper entirely. Clause-boundary chunking (EN/JA/KO/ZH markers), prosody extraction via autocorrelation, emotion→style mapping, adaptive endpointing (reconnects with tuned params after 5 utterances)
4. Pipeline RTMP integration — MP3→PCM decode, truncate+fadeout, source-language passthrough (zero API cost)
5. FFmpeg bundled as Tauri sidecar — static binary resolved at runtime next to executable
6. Voice cloning UI — dedicated card when live, 30s recording with progress bar, auto-activates cloned voice for all TTS
7. RTMP destination config — per-language URL inputs, source-language passthrough label

---

### Phase 2: Quality + Settings (DONE — Apr 2)

1. Settings UI — broadcast delay slider (1-10s), camera/mic device pickers via enumerateDevices(), collapsible panel
2. Session persistence — all config saved to localStorage (langs, tier, RTMP URLs, delay, device IDs), restored on launch
3. Device permission handling — brief getUserMedia on mount to unlock device labels before enumeration
4. Broadcast delay sent to backend via rtmp:config message, used by RtmpManager

---

### Phase 3: Production Polish (DONE — Apr 2)

1. Error banners — dismissible UI banners for WS disconnect, RTMP failures, backend errors
2. System tray — app minimizes to tray on window close, streaming continues in background
3. Adaptive endpointing fix — params were swapped, caused Deepgram 400 errors on reconnect

---

### Phase 4: Provider Swap (DONE — Apr 3)

1. Swapped Deepgram → Gladia Solaria-1 STT (faster transcription, better endpointing)
2. Swapped ElevenLabs model configuration, voice clone persistence to disk
3. Cleaned up API key naming to provider-agnostic (STT_API_KEY, TTS_API_KEY, TRANSLATE_API_KEY)

---

### Phase 5: Latency Optimization — Progressive Chunking (DONE — Apr 4)

**Three-pronged attack:**

1. **True Streaming TTS Decode** — `IncrementalMp3Decoder` in `ffmpeg.rs`. Long-lived FFmpeg subprocess decodes each MP3 chunk from ElevenLabs as it arrives. Pre-drains stdout to prevent pipe deadlock. Appends PCM to StreamingPcm immediately. **Saves 500-1500ms.**

2. **Progressive Clause Chunking** — `ProgressiveChunkDetector` in `stt.rs`. Splits utterances at clause boundaries during interims. Language-adaptive: EN 1000ms/2000ms, JA/KO 800ms/2000ms, ZH 800ms/2000ms. Tracks emitted text (not byte positions) for resilience to Gladia transcript revisions. Context-aware translation via `|||` separator.

3. **Chunked Pipeline Orchestration** — `spawn_chunked_pipeline()` in `pipeline.rs`. One StreamingPcm per (utterance, language). Multiple TTS chunks feed same buffer sequentially. Audio drain loop in `ffmpeg.rs` needed zero changes. Backpressure: skips chunks exceeding broadcast delay.

**Supporting changes:**
- Default broadcast delay reduced 5000ms → 3000ms
- Frontend handles `chunk_translation` messages with progressive append
- Audio accumulator cleared on STT reconnect
- Dropped chunk logging (channel full detection)

---

### Phase 6: Strategic Evaluation (NEXT — Apr 5+)

**Goal:** Evaluate whether to keep the custom pipeline or switch to end-to-end APIs.

1. **Evaluate Palabra AI** — end-to-end speech-to-speech translation via single WebSocket. If it delivers <2s latency with voice cloning, it replaces Gladia + Google Translate + ElevenLabs entirely (~60% of pipeline.rs).

2. **Evaluate Alibaba Qwen3-LiveTranslate-Flash-Realtime** — semantic unit prediction for SOV languages. May replace Gladia + Google Translate (keep ElevenLabs for voice cloning).

3. **Evaluate Soniox v4 / OpenAI semantic_vad** — as Gladia replacement for semantic endpointing. Keeps rest of pipeline intact.

4. **Decision criteria:**
   - Latency: <2s end-to-end per chunk
   - Voice cloning: must support host's cloned voice (not generic)
   - Languages: EN→JA, EN→KO, EN→ZH minimum
   - Cost: competitive with current ~$45/mo for 30 sessions
   - Reliability: <0.1% failure rate per utterance

---

### Phase 7: Demo Day (Weekend Apr 26-27)

**Goal:** Ship demo to Simon for Apr 30 deadline.

**Steps:**

1. **Test on Brivva's merchant accounts:**
   - Coupang (Korean stream)
   - Rakuten (Japanese stream)
   - YouTube (English passthrough)
   - Verify each platform receives correct language

2. **Record backup demo video** in case live demo has issues.

3. **Package:**
   - `.dmg` installer for macOS (`cargo tauri build`)
   - No prerequisites — single binary with bundled FFmpeg
   - Quick-start guide

4. **Prepare demo script:**
   - Host speaks English
   - 3 output streams: YouTube EN (passthrough), Coupang KR, Rakuten JP
   - Show voice cloning (30s sample → cloned voice)
   - Show reduced latency with progressive chunking
   - Show studio camera quality

---

## Open Questions

1. **End-to-end API evaluation** — Can Palabra/Qwen3 replace the custom pipeline? What's the latency/quality/cost tradeoff?
2. **macOS code signing** — unsigned app works for demo. Need Apple Developer account ($99/year) for distribution.
3. **Lipsync (Tiers 3-4)** — real-time and post-processed lipsync. Not production ready. Stretch goal after demo.
4. **H.264 passthrough** — `-c:v copy` would eliminate re-encode CPU. Needs either: (a) MediaRecorder outputting fragmented MP4 (not WebM), or (b) writing chunks to a temp file instead of stdin. Investigate after demo.

---

## File Structure

```
brivva/
├── Cargo.toml              ← workspace (server-rs, src-tauri)
├── .env.local              ← API keys (STT_API_KEY, TTS_API_KEY, TRANSLATE_API_KEY)
├── plan.md                 ← this file
├── problem.md              ← latency problem statement + strategic analysis
├── flow.md                 ← end-to-end flow documentation (needs update)
├── context.md              ← project context for AI collaborators
├── src-tauri/
│   ├── Cargo.toml          ← tauri v2 + tray-icon feature
│   ├── tauri.conf.json     ← externalBin: ffmpeg sidecar
│   ├── binaries/           ← ffmpeg-aarch64-apple-darwin (gitignored)
│   ├── src/main.rs         ← Tauri entry, system tray, spawns Axum server
│   └── icons/
├── server-rs/
│   ├── Cargo.toml          ← axum, tokio-tungstenite (native-tls), reqwest
│   └── src/
│       ├── lib.rs          ← Axum server + WebSocket handler (tagged binary routing)
│       ├── pipeline.rs     ← STT → progressive chunking → Translate → TTS → RTMP
│       ├── stt.rs          ← Gladia client, ProgressiveChunkDetector, prosody, emotion
│       ├── ffmpeg.rs       ← RTMP muxer: jitter buffer + IncrementalMp3Decoder + crash recovery
│       └── types.rs        ← Lang, Session, ChunkEvent, ServerMsg
└── frontend/
    ├── package.json
    └── src/
        ├── App.tsx
        ├── pages/
        │   └── BroadcastPage.tsx  ← config, settings, webcam, RTMP, voice clone, progressive transcript
        └── lib/
            ├── AudioPipeline.ts   ← mic capture (44.1kHz PCM, device selection)
            └── api.ts             ← REST client, platform configs, voice management
```

---

## Key Metrics for Demo

- **Sync quality:** <300ms audio-video offset (consistent, not variable)
- **Translation latency:** <3s end-to-end per chunk (speech → translated audio on stream)
- **Video quality:** 1080p minimum, higher with studio camera
- **Crash-free:** 2+ hour session without restart
- **Languages:** 3+ simultaneous (EN passthrough + JA + KO minimum)
- **Single binary:** No prerequisites, no Python, no system FFmpeg
