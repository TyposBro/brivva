# Brivva Desktop App — Implementation Plan

**Date:** April 2, 2026
**Target:** Working demo by April 30, 2026
**Constraint:** Weekend builds only (StoneLab weekdays)

---

## Architecture

**Fixed-delay jitter buffer** — one global delay D for all languages. App handles video + audio muxing internally via FFmpeg and pushes N RTMP streams directly to platforms. No OBS needed.

**Video pipeline:** Camera → MediaRecorder (hardware VP8/H.264) → encoded chunks → WebSocket (binary 0x02) → delayed buffer (D seconds) → FFmpeg stdin → re-encode libx264 ultrafast → FLV → RTMP

**Audio pipeline:** Mic → PCM 44.1kHz → WebSocket (binary 0x01) → Deepgram Nova-3 STT → Google Translate → ElevenLabs TTS → MP3→PCM → jitter buffer (D seconds) → FFmpeg audio FIFO → AAC → RTMP

**Key design decisions:**
- `-c:v copy` (passthrough) doesn't work with chunked WebM from MediaRecorder stdin — FFmpeg crashes. Always re-encode with `ultrafast`. CPU cost is low since input is already compressed.
- Platform latency does NOT absorb sync gaps — offset encoded = offset viewed
- Detection threshold: ~125ms audio-late, ~185ms annoying
- Variability is worse than consistent delay — brain adapts to fixed offset in ~30s
- Studio cameras (Elgato/BlackMagic capture cards) present as USB devices to getUserMedia

---

## Current State (end of Apr 2 session)

### Single binary — no external dependencies

- **Tauri v2 desktop app** — builds to `.app` + `.dmg` (macOS), system tray support
- **Backend:** 5 Rust files (~1800 lines) — `lib.rs`, `pipeline.rs`, `ffmpeg.rs`, `stt.rs`, `types.rs`
- **Frontend:** Single-page `BroadcastPage` (~500 lines) — tier selector, language config, RTMP destinations, settings (delay slider, device pickers), webcam preview, voice cloning, live transcript, error banners
- **FFmpeg:** Bundled as Tauri sidecar (77MB static binary, gitignored)
- **STT:** Direct Deepgram Nova-3 WebSocket in Rust (no Python wrapper). Clause-boundary chunking, prosody analysis, emotion classification, adaptive endpointing.
- **Video:** MediaRecorder with hardware encoder, any resolution/fps. Re-encoded via libx264 ultrafast for FLV/RTMP.
- **Protocol:** Tagged binary WebSocket (0x01=audio, 0x02=video) + JSON control messages (rtmp:config, video:codec, voice:sample)

---

## Implementation Phases

### Phase 1: Core Pipeline + Single Binary (DONE — Apr 2)

1. Restored `ffmpeg.rs` — jitter buffer, audio drain OS threads, RTMP push, crash recovery (3 retries), orphan cleanup
2. MediaRecorder video capture — hardware-encoded at any fps, tagged binary protocol (0x02)
3. Direct Deepgram STT in Rust — replaces Python stt-wrapper entirely. Clause-boundary chunking (EN/JA/KO/ZH markers), prosody extraction via autocorrelation, emotion→style mapping, adaptive endpointing (reconnects with tuned params after 5 utterances)
4. Pipeline RTMP integration — MP3→PCM decode, truncate+fadeout, source-language passthrough (zero API cost)
5. FFmpeg bundled as Tauri sidecar — static binary resolved at runtime next to executable
6. Voice cloning UI — dedicated card when live, 30s recording with progress bar, auto-activates cloned voice for all TTS (eleven_multilingual_v2 model)
7. RTMP destination config — per-language URL inputs, source-language passthrough label

---

### Phase 2: Quality + Settings (DONE — Apr 2)

1. Settings UI — broadcast delay slider (1-10s), camera/mic device pickers via enumerateDevices(), collapsible panel
2. Session persistence — all config saved to localStorage (langs, tier, RTMP URLs, delay, device IDs), restored on launch
3. Device permission handling — brief getUserMedia on mount to unlock device labels before enumeration
4. Broadcast delay sent to backend via rtmp:config message, used by RtmpManager

**Learned:** H.264 passthrough (`-c:v copy`) crashes FFmpeg with chunked WebM stdin. Always re-encode with ultrafast. CPU impact minimal since MediaRecorder already compressed.

---

### Phase 3: Production Polish (DONE — Apr 2)

1. Error banners — dismissible UI banners for WS disconnect, RTMP failures, backend errors. Backend sends ServerMsg::Error for RTMP start failures.
2. System tray — app minimizes to tray on window close, streaming continues in background. Tray icon with menu (Show Brivva / Quit). Click tray icon to restore window.
3. Adaptive endpointing fix — params were swapped (endpointing/utterance_end_ms), caused Deepgram 400 errors on reconnect. Fixed.

---

### Phase 4: Demo Day (Weekend Apr 26-27)

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
   - Show voice cloning (30s sample → cloned voice in your voice)
   - Show tier switching (subtitles only vs voice + subtitles)
   - Show studio camera quality

---

## Open Questions

1. **macOS code signing** — unsigned app works for demo. Need Apple Developer account ($99/year) for distribution.

2. **TTS provider evaluation** — Cartesia Sonic 3 (40ms TTFB, 3s clone) and Fish Audio (#1 TTS-Arena, 80% cheaper) are strong alternatives to ElevenLabs. Evaluate after demo.

3. **Lipsync (Tiers 3-4)** — real-time and post-processed lipsync. Not production ready. Stretch goal after demo.

4. **H.264 passthrough** — `-c:v copy` would eliminate re-encode CPU. Needs either: (a) MediaRecorder outputting fragmented MP4 (not WebM), or (b) writing chunks to a temp file instead of stdin. Investigate after demo.

---

## File Structure

```
brivva/
├── Cargo.toml              ← workspace (server-rs, src-tauri)
├── .env.local              ← API keys (DEEPGRAM, ELEVENLABS, GOOGLE_TRANSLATE)
├── plan.md                 ← this file
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
│       ├── pipeline.rs     ← Deepgram STT → Google Translate → ElevenLabs TTS → RTMP
│       ├── stt.rs          ← Deepgram client, chunking (4 langs), prosody, emotion, style
│       ├── ffmpeg.rs       ← RTMP muxer: video chunk drain + audio jitter buffer + crash recovery
│       └── types.rs        ← Lang, Session, ServerMsg
└── frontend/
    ├── package.json
    └── src/
        ├── App.tsx
        ├── pages/
        │   └── BroadcastPage.tsx  ← config, settings, webcam, RTMP, voice clone, transcript, errors
        └── lib/
            ├── AudioPipeline.ts   ← mic capture (44.1kHz PCM, device selection)
            └── RoomSocket.ts      ← WebSocket client (unused, kept for reference)
```

---

## Key Metrics for Demo

- **Sync quality:** <300ms audio-video offset (consistent, not variable)
- **Translation latency:** <3s end-to-end (speech → translated audio on stream)
- **Video quality:** 1080p minimum, higher with studio camera
- **Crash-free:** 2+ hour session without restart
- **Languages:** 3+ simultaneous (EN passthrough + JA + KO minimum)
- **Single binary:** No prerequisites, no Python, no system FFmpeg
