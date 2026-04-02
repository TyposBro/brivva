# Brivva Desktop App — Implementation Plan

**Date:** April 2, 2026
**Target:** Working demo by April 30, 2026
**Constraint:** Weekend builds only (StoneLab weekdays)

---

## Architecture

**Fixed-delay jitter buffer** — one global delay D for all languages. App handles video + audio muxing internally via FFmpeg and pushes N RTMP streams directly to platforms. No OBS needed.

**Video pipeline:** Camera → MediaRecorder (hardware H.264/VP8 encoder) → encoded chunks → WebSocket (tagged binary 0x02) → delayed buffer → FFmpeg stdin → FLV mux → RTMP

**Audio pipeline:** Mic → PCM (44.1kHz 16-bit mono) → WebSocket (tagged binary 0x01) → Deepgram Nova-3 STT → Google Translate → ElevenLabs TTS → MP3→PCM → jitter buffer → FFmpeg audio FIFO → AAC → RTMP

**Key constraints:**
- Platform latency does NOT absorb sync gaps — offset encoded = offset viewed
- Detection threshold: ~125ms audio-late, ~185ms annoying
- Variability is worse than consistent delay — brain adapts to fixed offset in ~30s

---

## Current State (end of Apr 2 session)

### Single binary — no external dependencies

- **Tauri v2 desktop app** — builds to `.app` + `.dmg` (macOS)
- **Backend:** 5 Rust files (~1500 lines) — `lib.rs`, `pipeline.rs`, `ffmpeg.rs`, `stt.rs`, `types.rs`
- **Frontend:** Single-page `BroadcastPage` (~400 lines) — tier selector, language config, RTMP destinations, webcam preview, voice cloning, live transcript
- **FFmpeg:** Bundled as Tauri sidecar (77MB static binary, gitignored)
- **STT:** Direct Deepgram Nova-3 WebSocket (no Python wrapper)
- **Video:** MediaRecorder with hardware encoder, any resolution/fps
- **Protocol:** Tagged binary WebSocket (0x01=audio, 0x02=video) + JSON control messages

---

## Implementation Phases

### Phase 1: Core Pipeline + Single Binary (DONE — Apr 2)

1. Restored `ffmpeg.rs` — jitter buffer, audio drain threads, RTMP push, crash recovery (3 retries), orphan cleanup
2. MediaRecorder video capture — hardware-encoded H.264/VP8 at any fps, tagged binary protocol
3. Direct Deepgram STT in Rust — clause-boundary chunking (EN/JA/KO/ZH), prosody extraction, emotion classification, adaptive endpointing
4. Pipeline RTMP integration — MP3→PCM decode, truncate+fadeout, source-language passthrough (zero API cost)
5. FFmpeg bundled as Tauri sidecar — resolved at runtime, no system install needed
6. Voice cloning UI — 30s recording, progress bar, auto-activates for all TTS
7. RTMP destination config — per-language URL inputs

---

### Phase 2: 4K + Quality Tuning (Weekend Apr 12-13)

**Goal:** Production-quality 4K60 video, tuned A/V sync.

**Steps:**

1. **H.264 passthrough** — test if WebKit MediaRecorder outputs H.264 (`video/mp4;codecs=avc1`). If yes, use `-c:v copy` in FFmpeg (zero CPU for video). If no, keep re-encode from VP8/VP9.

2. **4K capture** — request `{ width: 3840, height: 2160, frameRate: { ideal: 60 } }`. Test with studio capture card (Elgato/BlackMagic).

3. **Broadcast delay tuning:**
   - Measure P90/P95 TTS latency per language under real conditions
   - Set D = P95 + 200ms margin (start with 2.5s, tune down from 5s)
   - Add `BROADCAST_DELAY_MS` slider to settings UI

4. **Audio quality validation** — verify 44.1kHz PCM → AAC 128kbps output quality on RTMP streams.

5. **Local validation** — test with MediaMTX local RTMP server + `ffplay` before pushing to YouTube.

**Validation:** 4K60 stream to YouTube, A/V sync under 300ms.

---

### Phase 3: Production Polish (Weekend Apr 19-20)

**Goal:** Ready for real merchant account testing.

**Steps:**

1. **Settings UI:**
   - API keys (Deepgram, ElevenLabs, Google Translate) — stored in `.env.local`
   - Broadcast delay slider (`BROADCAST_DELAY_MS`)
   - Audio/video device selection (camera picker, mic picker)
   - Per-language RTMP destination config (already have inputs, add save/load)

2. **Error handling polish:**
   - FFmpeg crash recovery — already works (3 retries)
   - STT reconnect — already works (5 retries + adaptive endpointing)
   - TTS timeout — already works (D-500ms deadline)
   - Add clear error banners in UI (connection lost, API key invalid, etc.)

3. **System tray / background operation** — app continues streaming when window minimized.

4. **Session persistence** — save/load RTMP URLs and language config across app restarts.

**Validation:** Full end-to-end test — 4 languages, voice cloning, 2+ hour session without crash.

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
   - `.dmg` installer for macOS (Tauri builds this)
   - No prerequisites — single binary with bundled FFmpeg
   - Quick-start guide

4. **Prepare demo script:**
   - Host speaks English
   - 3 output streams: YouTube EN (passthrough), Coupang KR, Rakuten JP
   - Show voice cloning (30s sample → cloned voice)
   - Show tier switching (subtitles only vs voice + subtitles)
   - Show 4K quality with studio camera

---

## Open Questions

1. **macOS code signing** — unsigned app works for demo. Need Apple Developer account ($99/year) for distribution.

2. **TTS provider evaluation** — Cartesia Sonic 3 (40ms TTFB, 3s clone) and Fish Audio (#1 TTS-Arena, 80% cheaper) are strong alternatives to ElevenLabs. Evaluate after demo.

3. **Lipsync (Tiers 3-4)** — real-time and post-processed lipsync. Not production ready. Stretch goal after demo.

---

## File Structure

```
brivva/
├── Cargo.toml              ← workspace (server-rs, src-tauri)
├── .env.local              ← API keys (DEEPGRAM, ELEVENLABS, GOOGLE_TRANSLATE)
├── plan.md                 ← this file
├── src-tauri/
│   ├── Cargo.toml
│   ├── tauri.conf.json     ← externalBin: ffmpeg sidecar
│   ├── binaries/           ← ffmpeg-aarch64-apple-darwin (gitignored)
│   ├── src/main.rs         ← Tauri entry, spawns Axum server
│   └── icons/
├── server-rs/
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs          ← Axum server + WebSocket handler (tagged binary routing)
│       ├── pipeline.rs     ← Deepgram STT → Google Translate → ElevenLabs TTS → RTMP
│       ├── stt.rs          ← Deepgram client, chunking, prosody, emotion, style mapping
│       ├── ffmpeg.rs       ← RTMP muxer: video chunk buffer + audio jitter buffer
│       └── types.rs        ← Lang, Session, ServerMsg
└── frontend/
    ├── package.json
    └── src/
        ├── App.tsx
        ├── pages/
        │   └── BroadcastPage.tsx  ← all-in-one: config, webcam, RTMP, voice clone, transcript
        └── lib/
            ├── AudioPipeline.ts   ← mic capture (44.1kHz PCM)
            └── RoomSocket.ts      ← WebSocket client (unused, kept for reference)
```

---

## Key Metrics for Demo

- **Sync quality:** <300ms audio-video offset (consistent, not variable)
- **Translation latency:** <3s end-to-end (speech → translated audio on stream)
- **Video quality:** 1080p minimum, 4K60 with studio camera
- **Crash-free:** 2+ hour session without restart
- **Languages:** 3+ simultaneous (EN passthrough + JA + KO minimum)
- **Single binary:** No prerequisites, no Python, no system FFmpeg
