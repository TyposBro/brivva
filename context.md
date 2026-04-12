# Brivva — LLM Collaborator Context (Apr 13, 2026)

## What This Is

A Tauri v2 desktop app (Rust + React) for real-time multilingual live commerce broadcasting. One host speaks → N platforms receive translated audio in the host's cloned voice. Single binary, no cloud infra, FFmpeg bundled as sidecar.

## What Matters — Read This First

**This is NOT an MVP.** Brivva's business already works without this product — they use multiple influencers/studios per language. This app is a **cost optimization tool** that replaces 5 influencers with 1 host + tech. The quality bar is: "better than hiring 4 more humans." If the voice sounds robotic, stream drops, or sync is off — they're better off with humans and will say no.

**DEMO ON APRIL 16 (Wednesday), 1-4pm** with real Korean show hosts + videographer. This is the meeting that determines the partnership. Product must be flawless.

**Priority order for demo:**
1. Voice cloning quality — use ElevenLabs V2 model + 2-3 min voice sample (not 30s)
2. Reliability — 30+ min session, zero crashes
3. A/V sync perfection (<300ms offset)
4. Latency (important but secondary)

## Current State (v16.1 — Production Polish)

**Pipeline:**
```
Host Audio → N+1 Soniox v4 WebSocket connections (1 source transcript + 1 per target language)
           → Semantic endpointing (grammar-aware) + native translation (no external API)
           → 4-second force-chunk threshold for long monologues
           → Prosody analysis → emotion classification → voice style mapping
           → ElevenLabs TTS (WebSocket streaming + REST fallback)
           → StreamingPcm → FFmpeg audio drain (20ms ticks) → RTMP push

Host Video → MediaRecorder (hardware VP8/H.264) → tagged binary WS (0x02)
           → delayed buffer (broadcast_delay seconds) → FFmpeg → RTMP push

Source language → passthrough (host audio queued directly to RTMP, zero TTS cost)
```

**What needs to ship before Apr 16:**
1. Voice recording extended: 30s minimum → up to 3 min optional (longer sample = better clone)
2. 30-minute endurance test passed
4. Backup demo recording captured

**Architecture:** 69 Rust files, 4 layers (Core → Shared → Features → Orchestration). See `claude.md` for rules.

```
server-rs/src/
├── core/           # Pure types, config, audio utils, pipeline budget
├── shared/
│   ├── stt/        # Soniox v4 (10 files) — connection, handler, prosody, reconnect
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
- **N+1 connection strategy:** 1 source (transcript + interims) + 1 per target language (translation). Audio fanout to all.
- **Semantic endpointing:** Grammar-aware, not silence-based. `<end>` token with `is_final=true`.
- **Native translation:** `translation.type = "one_way"`, tokens arrive with `translation_status`. No external API.
- **Force chunking:** >4s without semantic endpoint → emit anyway.
- **Reconnect:** 5 attempts, 1s delay. Utterance counter preserved.

## TTS — ElevenLabs

- **Models available:**
  - `eleven_turbo_v2_5` — expressive, good quality (default)
  - `eleven_flash_v2_5` — fast, lower latency
- **Voice cloning:** 30s minimum sample, **extending to 2-3 min for higher quality**
  - Frontend: recording continues past 30s, "minimum reached" indicator, stop anytime up to 3 min
  - Backend: longer WAV → better clone fidelity
- **Streaming:** WebSocket primary, REST fallback. IncrementalMp3Decoder for real-time MP3→PCM.
- **TTFB:** ~75ms (Flash), ~300ms (Turbo/Expressive)

## Key Technical Details

- **Audio format:** 44.1kHz PCM, 16-bit mono
- **Video:** MediaRecorder hardware encoding → libx264 ultrafast → FLV → RTMP
- **Protocol:** Tagged binary WebSocket — 0x01=audio, 0x02=video. JSON for control messages.
- **Broadcast delay:** Default 3000ms, configurable 1-10s
- **Audio drain:** OS thread, 20ms ticks, 1764 bytes/tick. PCM → named FIFO → FFmpeg
- **Translation tiers:** tier 1 = subtitles only, tier 2 = voice + subtitles. Tiers 3-4 (lipsync) designed not implemented.
- **Crash recovery:** FFmpeg health monitor, 50 retries, 2s delay
- **Staleness eviction:** Audio queue items >6s old evicted. Queue depth limit 10.
- **Speech pacing:** Drain pads silence until original speech duration elapses after TTS finishes.
- **Drift tracking:** `Arc<AtomicU64>` per stream, `max_drift_ms()` API
- **Logging:** `tracing_subscriber` → stderr + `/tmp/brivva/server.log`

## Recently Fixed Bugs (v16.1, Apr 6)

- **FFmpeg init segment crash** — writes init segment before data chunks on first spawn
- **TTS cold-start 0 bytes** — retry once + REST fallback
- **TTS timeout at low delay** — 3s floor regardless of broadcast delay
- **Force-chunk stall** — fires on transcript duration, not translation presence
- **Audio pile-up** — caps stale play_at timestamps
- **Speech-duration pacing** — pads silence after short TTS to prevent audio outrunning video

## Pipeline is Commodity — Moat is Elsewhere

SOTA voice cloning is now open source (LongCat-AudioDiT). STT/translation providers are interchangeable. The pipeline will keep getting commoditized.

**The real engineering value** no API provides:
- RTMP muxer with fixed-delay jitter buffer
- Audio drain at 20ms ticks with staleness eviction + jitter recovery
- StreamingPcm accumulator (multiple TTS chunks → single buffer)
- FFmpeg crash recovery + health monitor
- A/V sync across delayed video + translated audio
- Source-language passthrough (zero API cost)
- N+1 connection orchestration with audio fanout

**Don't over-invest in pipeline sophistication.** Keep providers swappable. The streaming engine is the moat.

## Environment

- **API keys in `.env.local`:** `SONIOX_API_KEY`, `TTS_API_KEY`
- **Build:** `cargo tauri build` → `.app` + `.dmg`
- **Dev:** `cargo tauri dev` or `./dev.sh`
- **Architecture rules:** `claude.md` — 4-layer clean architecture

## Cost

| Service | Monthly (30 sessions) |
|---|---|
| Soniox v4 | ~$19 |
| ElevenLabs TTS | ~$250-350 |
| **Total** | **~$270-370** |

## Tone

Grounded, direct, technical. This is a production system for a business that generates ₩100M per contract. Ship quality, not features. When in doubt, ask — don't guess.
