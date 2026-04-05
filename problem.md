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

### FFmpeg crashes on first video chunk (P0 — FIXED)

**Root cause:** `trim_video_for_activation()` removed the fMP4 init segment (moov/trex) from the chunk buffer before FFmpeg spawned. FFmpeg saw moof fragments without a preceding trex → exit 183.

**Fix:** `video_drain.rs` — `write_init_segment_on_first_spawn()` polls for the init segment (stored in `video_init_segment: Arc<StdMutex<Option<Vec<u8>>>>`) and writes it to FFmpeg stdin before any data chunks. 30s timeout, 20ms poll interval.

### TTS returns 0 bytes on first calls (FIXED)

**Root cause:** ElevenLabs WebSocket cold-start. First 1-2 WS connections per session return empty audio.

**Fix:** `ws.rs` — `do_tts_ws()` retries once on 0-byte response. If retry also returns 0 bytes, returns `Err` to trigger REST fallback in `execute_tts_with_fallback()`.

### TTS timeouts at low broadcast delay (FIXED)

**Root cause:** TTS deadline = `broadcast_delay - 500ms`. At 1s delay → 500ms deadline, too short for ElevenLabs.

**Fix:** `config.rs` + `pipeline_budget.rs` — `TTS_DEADLINE_FLOOR_MS = 3000`. Formula: `max(min(delay - margin, cap), floor)`.

### Force-chunk stall during continuous speech (FIXED)

**Root cause:** `maybe_force_chunk()` in `handler.rs` required `translation_acc` to be non-empty before firing. Soniox doesn't emit incremental translation tokens during long continuous speech — it accumulates internally and dumps everything at the semantic endpoint. The 4s force-chunk threshold never triggered, causing 24s+ accumulation.

**Fix:** `handler.rs` — Force-chunk now fires based on transcript duration alone. If `translation_acc` is empty at force-chunk time, resets the timer without spawning TTS. When translation is available, emits normally.

### Audio pile-up on stale TTS results (FIXED)

**Root cause:** `play_at` was set to `utterance_start` (when host started speaking). After slow TTS generation, multiple queued items had stale `play_at` timestamps → all became "ready" simultaneously → played back-to-back → audio raced ahead of video.

**Fix:** `manager.rs` — `cap_stale_play_at()` caps `play_at` to `Instant::now()` when `utterance_start` is older than `broadcast_delay + 2s`. This adds a natural `broadcast_delay` gap before playing stale results.

### Translated audio outrunning video on asymmetric language pairs (FIXED)

**Root cause:** Translated speech can be shorter than original speech (KO→EN, counting, etc.). TTS generates 6s of audio for 10s of host speech → audio finishes early → next utterance starts immediately → audio progressively races ahead of video.

**Fix:** `types.rs` + `manager.rs` + `audio_drain.rs` — `QueuedAudio` now carries `speech_duration` (how long the host originally spoke). After TTS audio finishes playing, `schedule_speech_padding()` computes `remaining = speech_duration - audio_played`. If remaining > 0.5s, the drain pads silence (defers next utterance via `padding_until`) so audio stays synchronized with video regardless of translation length mismatch.

- For **shorter translations** (KO→EN, counting): silence padding fills the gap
- For **longer translations** (EN→JA): no padding needed; audio naturally runs slightly behind, self-corrects
- For **similar-length pairs**: minimal/no padding, existing sync preserved

### Subtitle overlay (drawtext) breaks YouTube — OPEN

**Symptom:** `-vf drawtext` causes YouTube "Preparing stream" indefinitely. Fontconfig error in bundled FFmpeg.

**Status:** Disabled by default (`BRIVVA_SUBTITLES=1` to enable). Needs fontconfig bundling or alternative subtitle approach.

## Known Limitation: Soniox Translation Accumulation

Soniox accumulates translation for long continuous speech and emits it all at the semantic endpoint. Force-chunking resets the local `translation_acc` every 4s, but Soniox's internal translation state keeps accumulating. The translation tokens only exist when Soniox decides to emit them.

**Mitigations in place:**
- Speech-duration pacing pads silence after short TTS audio to match original speech length
- Stale `play_at` capping prevents pile-up of multiple TTS results
- Force-chunk on transcript duration prevents 24s+ accumulations

**Residual impact:** For pathological inputs (slowly counting numbers), the translated audio may be significantly shorter than original speech. Pacing helps but can't fully bridge the gap when Soniox emits 17s of accumulated translation in one burst. Not an issue for natural commerce speech with normal pauses.
