# Real-Time Translated Live Commerce Broadcasting — Technical Design

## What We're Building

A desktop app that enables a single live commerce host to broadcast simultaneously in multiple languages. The host speaks naturally in one language. The app translates their speech into N target languages using their cloned voice, so viewers on each platform hear the same person speaking their language. Each platform (YouTube, Coupang, Rakuten) receives a synced video+audio stream in one language.

The goal is to feel as close to a human simultaneous interpreter as possible — but with the host's own cloned voice instead of an interpreter's voice.

## Current Pipeline (v16 — Soniox v4, Complete)

```
Host Audio → N+1 Soniox v4 WebSocket connections
               ├─ Connection 0: source language → transcript (interim updates to frontend)
               ├─ Connection 1: source → target_lang[0] (translation)
               └─ Connection N: source → target_lang[N-1] (translation)
           → Semantic endpointing (<end> token) OR 4-second force-chunk
           → Prosody analysis → emotion classification → voice style mapping
           → ElevenLabs Flash v2.5 TTS (WebSocket streaming + REST fallback)
           → IncrementalMp3Decoder (streaming MP3→PCM)
           → StreamingPcm accumulator
           → FFmpeg audio drain (20ms ticks) → named FIFO → RTMP push

Host Video → MediaRecorder (hardware VP8/H.264)
           → tagged binary WebSocket (0x02)
           → delayed buffer (broadcast_delay seconds)
           → FFmpeg stdin → libx264 ultrafast → FLV → RTMP push

Source language → passthrough (host audio queued directly to RTMP, zero TTS cost)
```

**Cost:** ~$270-370/month (Soniox $19 + ElevenLabs $250-350).

## Pipeline Evolution

### v14 — Full-sentence translation
STT waits for complete utterance → translate entire sentence → TTS. 2-10 second gaps for long sentences.

### v15 — Progressive chunking (replaced)
Custom ProgressiveChunkDetector split utterances at clause boundaries during interims. Language-specific markers (EN/JA/KO/ZH). Context-aware translation via `|||` separator. ~595 lines of custom chunking code.

**Problems:** Marker-based detection failed when host rambled. Force-split could cut mid-word. SOV languages needed special handling. Pipeline overhead accumulated and caused drift.

### v15.1 — Resilience hardening
Translation timeout, TTS finish() guarantee, staleness eviction, queue depth limits, drift tracking, auto-reconnect, pipeline health monitoring.

### v16 — Soniox v4 migration (current)
Replaced Gladia + Google Translate + ProgressiveChunkDetector with Soniox v4. ~595 lines of custom chunking deleted, replaced by 25-line force-chunk threshold. N+1 per-language connections with audio fanout. Native translation built into STT WebSocket.

## Why Soniox v4

| Dimension | Old (Gladia+Google) | Current (Soniox v4) |
|---|---|---|
| STT+Translation | 2 services, ~400-700ms | 1 WebSocket, ~300-500ms |
| Endpointing | Silence-based + custom chunker | Semantic (grammar-aware) |
| JA WER | ~12-15% | 8.7% |
| KO WER | ~8-12% | 4.3% |
| Cost (STT+Translation) | ~$168/mo | ~$19/mo |
| Chunking code | ~595 lines (markers, detectors, context handling) | ~25 lines (4s force threshold) |
| Translation | Separate Google API call | Native in STT response |
| Connection model | 1 WebSocket | N+1 (1 per target language) |

### Eliminated alternatives

- **Qwen3-LiveTranslate:** No voice cloning (8 preset voices only). 3s latency. Python SDK only.
- **OpenAI Realtime API:** Turn-based (incompatible with continuous broadcast). $3,200-9,800/mo.
- **Google DeepMind S2ST:** Not available as API. No CJK. Validates our 3s delay approach.
- **Palabra/Pinch:** $35,000/mo for 4 languages.
- **LongCat-AudioDiT:** SOTA voice cloning (open source) but batch-only, needs GPU. Not real-time viable. Relevant for post-processed Option 4.

## Key Design Decisions

1. **N+1 connections over single multiplexed connection.** Each target language gets its own Soniox WebSocket with dedicated translation config. Simpler error isolation. Audio fanout via broadcast channel.

2. **4-second force-chunk threshold.** If Soniox semantic endpointing doesn't fire within 4s, emit whatever translation has accumulated. Prevents infinite accumulation during monologues. Simple duration check, not language-specific heuristics.

3. **Source-language passthrough.** Host audio queued directly to RTMP manager — no STT, no TTS, zero API cost. Source platform gets the real voice.

4. **Prosody → emotion → voice style.** Pitch analysis (autocorrelation), energy RMS, pause density → classify into 9 emotions → map to ElevenLabs voice settings (speed, style). Translated speech inherits the host's emotional tone.

5. **TTS WebSocket primary, REST fallback.** WebSocket streaming gives ~75ms TTFB. REST fallback if WS connection fails. 10s timeout cap on all TTS calls.

6. **Pipeline is commodity, streaming engine is moat.** STT/TTS providers are swappable (just proved it by swapping Gladia→Soniox in one session). The RTMP muxer, jitter buffer, A/V sync, and crash recovery — that's the engineering no API provides.

## Bugs Fixed (Historical)

### v16 jitter recovery bug
Audio drain's jitter recovery incremented byte counters but never wrote silence to FIFO. FFmpeg audio FIFO starved → muxer stalled → both video AND audio stopped → YouTube stream paused. Fix: recovery now writes real data (available audio first, then silence).

### v15 progressive chunking drift
Multiple TTS chunks for same utterance could accumulate pipeline overhead, causing audio to fall behind video. Staleness eviction (>6s) + queue depth limit (10) + drift tracking mitigated this.

## Constraints

- Desktop app (Tauri + Rust), runs locally, no cloud GPU
- ElevenLabs Flash v2.5 for TTS (may be replaced when better real-time voice cloning APIs emerge)
- Must maintain voice cloning quality (host's voice, not generic)
- Target: **<2 second average end-to-end latency** (speech → translated audio on stream)
- Must handle 4+ languages simultaneously
- Default broadcast delay: 3000ms
- Quality bar: "better than hiring 4 more human interpreters"

## Bugs Fixed (Apr 6, 2026)

### Bug: FFmpeg crashes on first video chunk (P0 — FIXED)

**Symptom:** Every session, FFmpeg crashes immediately on first video data. Auto-restarts and works after, but ~5s of video is lost. Audio keeps flowing → permanent ~5s audio-ahead-of-video desync. Confirmed by counting with fingers on camera at 1s broadcast delay.

**FFmpeg error:**
```
could not find corresponding trex (id 1)
error reading header
Error opening input file pipe:0
Process crashed for lang=ru, exit=183
```

**Root cause:** MediaRecorder outputs fragmented MP4 (fMP4). FFmpeg needs the init segment (moov atom with trex boxes) before it can parse any moof/trun fragments. The video drain starts writing data chunks to FFmpeg's stdin before the init segment arrives. FFmpeg sees a trun box without a preceding trex → crashes.

**Evidence:**
- On restart, `replaying init segment (929431B)` is logged — the init segment IS captured and available
- After restart, FFmpeg works perfectly — init segment is written first
- The bug is that first-spawn doesn't wait for init segment before piping data

**Fix (Apr 6):** `video_drain.rs` — Added `write_init_segment_on_first_spawn()` that polls for the init segment and writes it to FFmpeg stdin before any data chunks. The deeper root cause: `trim_video_for_activation()` was removing the init segment from the chunk buffer because it's older than `broadcast_delay` by the time first audio arrives. Now both first-spawn and restart paths write the init segment first.

### Bug: TTS returns 0 bytes on first 1-2 calls (FIXED)

**Symptom:** First 1-2 ElevenLabs TTS WebSocket calls per session return empty audio (0 bytes). Subsequent calls work fine.

**Fix (Apr 6):** `ws.rs` — Extracted `do_tts_ws_once()` and added automatic retry in `do_tts_ws()` when the first attempt returns 0 audio bytes.

### Bug: TTS timeouts at low broadcast delay (FIXED)

**Symptom:** At 1s broadcast delay, TTS deadline becomes 500ms (broadcast_delay - 500ms). ElevenLabs Turbo v2.5 often exceeds this, causing TIMEOUT.

**Fix (Apr 6):** `config.rs` + `pipeline_budget.rs` — Added `TTS_DEADLINE_FLOOR_MS = 3000`. `compute_tts_deadline()` now returns `max(min(delay - margin, cap), floor)`. Decouples TTS generation budget from broadcast delay.

### Subtitle overlay (drawtext) breaks YouTube streaming

**Symptom:** Adding `-vf drawtext` to FFmpeg args causes YouTube to show "Preparing stream" indefinitely. Stream health shows "Excellent" but video never appears.

**Likely cause:** drawtext filter + Fontconfig error (`Cannot load default config file: No such file`) produces output YouTube can't parse. Or the decode→filter→re-encode pipeline changes the H.264 output in a way YouTube rejects.

**Status:** Disabled by default. Enable with `BRIVVA_SUBTITLES=1` env var. Needs investigation — may need to bundle fontconfig, or use a different subtitle approach.
