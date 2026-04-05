# Brivva — LLM Collaborator Context (Apr 5, 2026)

## What This Is

A Tauri v2 desktop app (Rust + React) for real-time multilingual live commerce broadcasting. One host speaks → N platforms receive translated audio in the host's cloned voice. Single binary, no cloud infra, FFmpeg bundled as sidecar.

## What Matters — Read This First

**This is NOT an MVP.** Brivva's business already works without this product — they use multiple influencers/studios per language. This app is a **cost optimization tool** that replaces 5 influencers with 1 host + tech. The quality bar is: "better than hiring 4 more humans." If the voice sounds robotic, stream drops, or sync is off — they're better off with humans and will say no.

**Don't demo until it's premium.** Half-working demo hurts more than no demo. Target: 30+ min live session with zero hiccups.

**Priority order:**
1. Voice cloning quality (robotic = dead product)
2. Reliability (2hr sessions, zero crashes)
3. A/V sync perfection (<300ms offset)
4. Latency (important but secondary to quality)

**Focus one language pair first:** Korean host → Japanese output (Coupang KR → Rakuten JP). That's the highest-value stream.

## Current State (v16 — Soniox Migration Complete)

**Pipeline:**
```
Host Audio → N+1 Soniox v4 WebSocket connections (1 source transcript + 1 per target language)
           → Semantic endpointing (grammar-aware) + native translation (no external API)
           → 4-second force-chunk threshold for long monologues
           → Prosody analysis → emotion classification → voice style mapping
           → ElevenLabs Flash v2.5 TTS (WebSocket streaming + REST fallback)
           → StreamingPcm → FFmpeg audio drain (20ms ticks) → RTMP push

Host Video → MediaRecorder (hardware VP8/H.264) → tagged binary WS (0x02)
           → delayed buffer (broadcast_delay seconds) → FFmpeg → RTMP push

Source language → passthrough (host audio queued directly to RTMP, zero TTS cost)
```

**What was removed in v16:**
- Gladia STT (zero references remain)
- Google Translate (zero references remain)
- ProgressiveChunkDetector, MarkerDetector, FallbackDetector (zero references remain)
- Clause marker tables (EN/JA/KO/ZH)
- `|||` context separator logic
- `derive_position` / revision handling
- ~595 lines deleted, replaced by ~25 lines of duration-based force chunking

**Architecture:** 69 Rust files, 4 layers (Core → Shared → Features → Orchestration). See `claude.md` for rules.

```
server-rs/src/
├── core/           # Pure types, config, audio utils, pipeline budget
├── shared/
│   ├── stt/        # Soniox v4 (10 files, ~2037 lines) — connection, handler, prosody, reconnect
│   ├── tts/        # ElevenLabs (6 files) — WebSocket streaming + REST fallback
│   └── voice_clone/ # ElevenLabs clone API + disk persistence
├── features/broadcast/
│   ├── domain/     # Session, messages, pipeline budget
│   └── data/       # WebSocket handler, RTMP streaming (11 files), pipeline helpers, voice API
├── orchestration/  # DI, config, router
├── lib.rs          # run_server() + logging + env loading
└── main.rs         # Entry point
```

## Soniox v4 Integration Details

- **WebSocket:** `wss://stt-rt.soniox.com/transcribe-websocket`
- **Model:** `stt-rt-v4`
- **API key:** `SONIOX_API_KEY` env var
- **N+1 connection strategy:** 1 source (provides transcript + interim updates) + 1 per target language (provides translation). Audio broadcast from host to all connections.
- **Semantic endpointing:** Soniox detects grammar completion, not just silence. `<end>` token with `is_final=true` triggers endpoint.
- **Native translation:** `translation.type = "one_way"`, `target_language` in config. Tokens arrive with `translation_status: "original" | "translation"`. No external API needed.
- **Force chunking:** If >4 seconds without semantic endpoint, emit accumulated translation anyway. Prevents infinite accumulation during monologues.
- **Max endpoint delay:** 1500ms
- **Keepalive:** 15-second interval
- **Reconnect:** Up to 5 attempts, 1-second delay. Utterance counter preserved across reconnections.

## Prosody & Emotion Pipeline

Extracts from host audio per utterance:
- Pitch (autocorrelation, 50-500 Hz range): mean + std deviation
- Energy RMS (loudness)
- Pause density (hesitation detection)
- Speaking rate (WPM)

Maps to 9 emotions → TTS voice style + speed:
- excited (loud + expressive + high pitch) → fast
- angry (loud + expressive) → intense
- happy, sad, serious, neutral, etc.

## Key Technical Details

- **Audio format:** 44.1kHz PCM, 16-bit mono
- **Video:** MediaRecorder hardware encoding, re-encoded via libx264 ultrafast for FLV/RTMP
- **Protocol:** Tagged binary WebSocket — 0x01=audio, 0x02=video. JSON for control messages.
- **Broadcast delay:** Default 3000ms, configurable 1-10s
- **Audio drain:** OS thread, 20ms ticks, 1764 bytes/tick. PCM → named FIFO → FFmpeg
- **TTS:** ElevenLabs Flash v2.5, WebSocket streaming primary, REST fallback. IncrementalMp3Decoder for real-time MP3→PCM. ~75ms TTFB. 10s timeout cap.
- **Translation tiers:** tier 1 = subtitles only, tier 2 = voice + subtitles. Tiers 3-4 (lipsync) not implemented.
- **Crash recovery:** FFmpeg health monitor, 50 retries, 2s delay
- **Staleness eviction:** Audio queue items >6s old evicted. Queue depth limit 10.
- **Drift tracking:** `Arc<AtomicU64>` per stream, `max_drift_ms()` API
- **Logging:** `tracing_subscriber` → stderr + `/tmp/brivva/server.log`

## Recently Fixed Bugs (Apr 6)

- **V1 (P0): FFmpeg init segment crash** — `video_drain.rs` now writes init segment to stdin before any data chunks. Root cause: `trim_video_for_activation()` was removing it from the buffer. Fixed for both first-spawn and restart paths.
- **V2: TTS cold-start 0 bytes** — `ws.rs` auto-retries when first ElevenLabs call returns empty audio.
- **V3: TTS timeout at low delay** — `TTS_DEADLINE_FLOOR_MS = 3000` ensures minimum 3s TTS budget regardless of broadcast delay.

**Known issue:** Subtitle overlay (drawtext) disabled by default (`BRIVVA_SUBTITLES=1`). Breaks YouTube streaming (Fontconfig missing in bundled FFmpeg).

## Pipeline is Commodity — Moat is Elsewhere

SOTA voice cloning is now open source (LongCat-AudioDiT, SIM 0.818). STT/translation providers are interchangeable (just swapped Gladia+Google for Soniox in one session). The pipeline will keep getting commoditized.

**The real engineering value** that no TTS/STT API provides:
- RTMP muxer with fixed-delay jitter buffer
- Audio drain at 20ms ticks with staleness eviction + jitter recovery
- StreamingPcm accumulator (multiple TTS chunks → single buffer)
- FFmpeg crash recovery (50 retries, health monitor)
- A/V sync across delayed video + translated audio
- Source-language passthrough (zero API cost)
- N+1 connection orchestration with audio fanout

**Don't over-invest in pipeline sophistication.** Keep STT/TTS providers swappable. The streaming engine is what matters.

## Remaining Work (from todo.md)

**Priority 2 — Resilience (post-migration):**
- Empty translation guard
- Skip-ahead logic (drift > 1.5x broadcast_delay → skip to newest)
- Pipeline failure counters (replace hardcoded 0s)
- E2E latency tracking (chunk_start → TTS_complete, rolling average)
- Circuit breaker for TTS + Soniox APIs
- Frontend health dashboard (per-language stream status, green/yellow/red)
- Graceful degradation (auto subtitle-only when drift > threshold)
- Error deduplication (5s window)

## Environment

- **API keys in `.env.local`:** `SONIOX_API_KEY`, `TTS_API_KEY`, `TRANSLATE_API_KEY` (legacy, unused)
- **Build:** `cargo tauri build` → `.app` + `.dmg`
- **Dev:** `cargo tauri dev` or `./dev.sh`
- **Architecture rules:** `claude.md` — 4-layer clean architecture, no sibling imports, env vars in orchestration only

## Cost

| Service | Monthly (30 sessions) |
|---|---|
| Soniox v4 | ~$19 |
| ElevenLabs TTS | ~$250-350 |
| **Total** | **~$270-370** |

Down from ~$400-540/mo (Gladia + Google Translate + ElevenLabs). Saves ~$150/mo.

## Tone

Grounded, direct, technical. This is a production system for a business that generates ₩100M per contract. Ship quality, not features. When in doubt, ask — don't guess.
