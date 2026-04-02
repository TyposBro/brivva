# Brivva Desktop App — Implementation Plan

**Date:** April 2, 2026
**Target:** Working demo by April 30, 2026
**Constraint:** Weekend builds only (StoneLab weekdays)

---

## Architecture Decision

**Fixed-delay jitter buffer (Option 2)** — one global delay D for all languages. The app handles video + audio muxing internally via FFmpeg and pushes N RTMP streams directly to platforms. No multiple OBS instances needed.

**Why Option 2 over Option 3 (per-language delay):**

- Already built and proven in v12's `ffmpeg.rs`
- Difference is ~500ms latency savings on fast languages — not worth the complexity
- One video delay pipeline vs N — critical for single developer shipping by Apr 30
- All three second opinions (ChatGPT, Gemini, Perplexity) confirmed jitter buffer is mandatory

**Key insight from second opinions:**

- Platform latency does NOT absorb sync gaps — offset encoded = offset viewed
- Detection threshold: ~125ms audio-late, ~185ms annoying. 1-3s is broken.
- Variability is worse than consistent delay — brain adapts to fixed offset in ~30s

---

## Current State (end of Apr 2 session)

### What exists and works

- Tauri v2 desktop app builds to `.app` + `.dmg` (macOS)
- Stripped backend: 3 files (~450 lines) — `lib.rs`, `pipeline.rs`, `types.rs`
- WebSocket protocol: `/ws?sourceLang=en&targetLangs=ja,ko&tier=2`
- Pipeline: STT (Deepgram) → Translate (Google) → TTS (ElevenLabs)
- Tier support: tier 1 (subtitles only), tier 2 (voice + subtitles)
- Frontend: single-page BroadcastPage with tier selector, language config, live transcript
- Voice cloning works (ElevenLabs Instant Voice Clone)

### What was removed (from v12)

- `ffmpeg.rs` — RTMP muxing, jitter buffer, A/V sync ← **BRINGING THIS BACK**
- `youtube.rs` — not needed (OBS/manual RTMP)
- `routes.rs`, `db.rs` — not needed
- `platform.rs` — not needed
- `room/handler.rs` — replaced with simplified WebSocket handler
- All AWS infrastructure — deleted, not needed

### What still exists in git history

- Full `ffmpeg.rs` with jitter buffer, video/audio drain threads, crash recovery
- Can be restored from git and adapted for desktop

---

## Implementation Phases

### Phase 1: Restore FFmpeg Muxing (Weekend Apr 5-6)

**Goal:** App captures webcam + mic, translates, and pushes synced RTMP streams locally.

**Steps:**

1. **Restore `ffmpeg.rs` from git** — the core jitter buffer, video/audio drain threads, RTMP push logic. Adapt:

   - Remove AWS/ECS-specific code
   - Remove CloudWatch metric logging
   - Keep: `RtmpManager`, `push_video_frame()`, `queue_audio()`, `truncate_with_fadeout()`, `decode_mp3_to_pcm()`, video/audio drain OS threads, crash recovery
   - FFmpeg must be installed on the host machine (document as prerequisite)

2. **Add webcam capture to the backend** — two approaches:

   - **Option A (simpler):** Keep browser-based capture via WebSocket (current approach). Frontend captures webcam frames via canvas → sends as binary to backend. Backend feeds frames to FFmpeg. This already worked in v12.
   - **Option B (better perf):** Native webcam capture in Rust via `nokhwa` crate. Eliminates WebSocket overhead for video. But adds complexity.
   - **Decision:** Option A for demo. Optimize to Option B later if needed.

3. **Update `pipeline.rs`** — re-add RTMP audio queueing:

   - After TTS completes, decode MP3 → PCM via `ffmpeg::decode_mp3_to_pcm()`
   - Truncate + fadeout if exceeds utterance duration + 2s
   - Queue to `RtmpManager` via `queue_audio(lang, pcm, utterance_start)`
   - Re-add source-language passthrough (host audio → RTMP, zero API cost)

4. **Update `types.rs`** — re-add to Session:

   - `frame_buffer: FrameBuffer` (ring buffer of timestamped video frames)
   - `rtmp_manager: Option<SharedRtmpManager>`
   - `rtmp_langs: Vec<Lang>` (which languages have RTMP streams)

5. **Update WebSocket handler (`lib.rs`)** — re-add:

   - Video frame reception (`face:frame` JSON messages)
   - Push frames to `session.frame_buffer` and to `rtmp_manager.push_video_frame()`
   - RTMP stream lifecycle: start FFmpeg processes when session starts, stop on disconnect

6. **Update frontend** — re-add:
   - Webcam capture (canvas → base64 JPEG → WebSocket)
   - RTMP destination config: for each target language, user enters RTMP URL + stream key
   - Source language gets passthrough (no TTS cost)

**RTMP config in frontend:**

```
Target Languages:
  🇯🇵 Japanese → rtmp://... + stream key (for Rakuten)
  🇰🇷 Korean   → rtmp://... + stream key (for Coupang)
  🇬🇧 English  → passthrough, rtmp://... + stream key (for YouTube)
```

**Validation:** Start session → speak → see translated audio + synced video arrive on a local RTMP test server (MediaMTX) → verify with `ffplay`.

---

### Phase 2: 4K + Quality (Weekend Apr 12-13)

**Goal:** Production-quality video output.

**Steps:**

1. **4K webcam capture** — request `{ video: { width: { ideal: 3840 }, height: { ideal: 2160 } } }` from getUserMedia. Test in Tauri WebView.

2. **FFmpeg encoding optimization:**

   - Keep CRF 20 + `ultrafast` + `zerolatency` for software encoding
   - Test NVENC if GPU available: `-c:v h264_nvenc -preset p1 -tune ll`
   - 4K × N streams with software encoding will be CPU-heavy — may need to fall back to 1080p for >4 languages

3. **Broadcast delay tuning:**

   - Measure P90/P95 TTS latency per language under real conditions
   - Set D = P95 + 200ms margin (start with 2.5s, tune down)
   - Add `BROADCAST_DELAY_MS` to frontend settings UI

4. **Audio quality** — keep 44.1kHz, 16-bit, mono. AAC output at 128kbps.

**Validation:** 4K stream to YouTube via RTMP, verify quality matches native OBS output.

---

### Phase 3: Production Polish (Weekend Apr 19-20)

**Goal:** Ready for real merchant account testing.

**Steps:**

1. **Settings UI:**

   - API keys (Deepgram, ElevenLabs, Google Translate)
   - Broadcast delay slider
   - Audio/video device selection
   - Per-language RTMP destination config (URL + key)

2. **Error handling:**

   - FFmpeg crash recovery (3 retries, already implemented in v12)
   - STT reconnect (5 retries, already works)
   - TTS timeout (hard deadline at D-500ms, already implemented)
   - Clear error messages in UI

3. **Voice cloning UX:**

   - Record 30s voice sample
   - Clone → use cloned voice for all TTS
   - Show status in UI

4. **System tray / background operation** — app continues running during live stream even if window is minimized.

**Validation:** Full end-to-end test with all 4 languages, voice cloning, 2+ hour session.

---

### Phase 4: Demo Day

**Goal:** Ship demo to Simon for Apr 20 deadline.

**Steps:**

1. **Test on Brivva's merchant accounts:**

   - Coupang (Korean stream)
   - Rakuten (Japanese stream)
   - YouTube (English stream)
   - Verify each platform receives correct language

2. **Record backup demo video** in case live demo has issues.

3. **Package:**

   - `.dmg` installer for macOS
   - README with prerequisites (FFmpeg, Python for stt-wrapper)
   - Quick-start guide

4. **Prepare demo script:**
   - Host speaks English
   - 3 output streams: YouTube EN (passthrough), Coupang KR, Rakuten JP
   - Show voice cloning
   - Show tier switching (subtitles only vs voice + subtitles)
   - Show 4K quality

---

## Open Questions

1. **stt-wrapper dependency** — currently requires Python. Options:

   - (a) Ship with Python as prerequisite (fine for demo)
   - (b) Bundle Python via PyInstaller (heavy)
   - (c) Rewrite in Rust (cleanest, but takes time)
   - **Decision:** (a) for demo, (c) as follow-up

2. **OBS vs internal FFmpeg** — with internal FFmpeg muxing, OBS is no longer needed for the core workflow. OBS becomes optional (for overlays, scene switching, monitoring). The app IS the streaming engine.

3. **Multiple simultaneous 4K streams** — may need NVENC or resolution fallback. Test on Phase 2 weekend.

4. **macOS code signing** — unsigned app works for demo. Need Apple Developer account for distribution.

---

## File Structure (Target)

```
brivva/
├── Cargo.toml              ← workspace
├── .env.local              ← API keys (localhost config)
├── plan.md                 ← this file
├── brivva-context.md       ← project context for AI collaborator
├── src-tauri/
│   ├── Cargo.toml
│   ├── tauri.conf.json
│   ├── src/main.rs         ← Tauri entry, spawns Axum
│   └── icons/
├── server-rs/
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs          ← Axum server + WebSocket handler
│       ├── pipeline.rs     ← STT → Translate → TTS + RTMP queueing
│       ├── ffmpeg.rs       ← RESTORED: jitter buffer, RTMP muxing, drain threads
│       └── types.rs        ← Lang, Session, ServerMsg, FrameBuffer
├── stt-wrapper/            ← Python Deepgram proxy (sidecar)
└── frontend/
    ├── package.json
    ├── vite.config.ts
    └── src/
        ├── App.tsx
        ├── pages/
        │   └── BroadcastPage.tsx  ← single page: config + transcript + RTMP destinations
        └── lib/
            ├── AudioPipeline.ts   ← mic capture
            └── RoomSocket.ts      ← WebSocket client (simplified)
```

---

## Key Metrics for Demo

- **Sync quality:** <300ms audio-video offset (consistent, not variable)
- **Translation latency:** <3s end-to-end (speech → translated audio on stream)
- **Video quality:** 1080p minimum, 4K if hardware supports
- **Crash-free:** 2+ hour session without restart
- **Languages:** 3+ simultaneous (EN passthrough + JA + KO minimum)
