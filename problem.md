# Real-Time Translated Live Commerce Broadcasting — Latency Optimization

## What We're Building

A desktop app that enables a single live commerce host to broadcast simultaneously in multiple languages. The host speaks naturally in one language. The app translates their speech into N target languages using their cloned voice, so viewers on each platform hear the same person speaking their language. Each platform (YouTube, Coupang, Rakuten) receives a synced video+audio stream in one language.

The goal is to feel as close to a human simultaneous interpreter as possible — but with the host's own cloned voice instead of an interpreter's voice.

## Pipeline Evolution

### v15 — Progressive Chunking (current, being replaced)

```
Host speaks → Gladia STT (real-time transcription)
           → ProgressiveChunkDetector (clause-boundary splitting during speech)
           → Google Cloud Translation API v2 (~130-376ms, context-aware with ||| separator)
           → ElevenLabs Flash v2.5 TTS (voice-cloned, incremental MP3→PCM streaming)
           → Single StreamingPcm accumulator per (utterance, language)
           → FFmpeg muxes translated audio + delayed video
           → RTMP push to platform
```

**Latency:** ~2.3-3s end-to-end. First audio at ~5.3s for 8s utterance (3 chunks).
**Cost:** ~$400-540/month (Gladia $88 + Google Translate $80 + ElevenLabs $250-350).

### v16 — Soniox v4 Migration (next)

```
Host speaks → Soniox v4 STT+Translation (semantic endpointing, streaming translation)
           → ElevenLabs Flash v2.5 TTS (voice-cloned, incremental MP3→PCM streaming)
           → Single StreamingPcm accumulator per (utterance, language)
           → FFmpeg muxes translated audio + delayed video
           → RTMP push to platform
```

**Expected latency:** ~1.5-2s end-to-end. Eliminates Google Translate hop (200-300ms) and custom chunking overhead.
**Expected cost:** ~$270-370/month (Soniox $19 + ElevenLabs $250-350). Saves ~$150/month.

### What Soniox v4 eliminates

| Removed component | Lines | Why |
|---|---|---|
| `ProgressiveChunkDetector` | ~225 | Soniox semantic endpointing replaces clause-boundary splitting |
| `MarkerDetector` / `FallbackDetector` | ~55 | Language-specific marker tables no longer needed |
| `markers.rs` | ~100 | EN/JA/KO/ZH clause marker definitions |
| `Google Translate module` | ~145 | Soniox has built-in streaming translation |
| Context separator (`\|\|\|`) logic | ~30 | No inter-chunk context needed — semantic endpointing handles it |
| `derive_position` / revision handling | ~40 | Soniox token protocol handles revisions differently |

### What stays unchanged

- `StreamingPcm` + audio drain (20ms ticks, jitter recovery, staleness eviction)
- `RtmpManager` + FFmpeg muxing + health monitor
- Video drain + broadcast delay jitter buffer
- ElevenLabs TTS (WebSocket streaming + REST fallback)
- Frontend (broadcast page, controls, transcript display)
- Voice cloning pipeline

## The Latency Problem (original)

The bottleneck was **sentence chunking**. STT must wait for the speaker to finish a thought before translating, because partial sentences produce bad translations.

**Real speech patterns:**

- Short phrase: "This is 50% off" → 2 seconds → fast
- Long sentence: "This product was shipped from Colombia and sold out within one month of launch, and right now we're offering buy one get one free" → 8-10 seconds → viewer waits the entire time before hearing anything

### v15 solution: ProgressiveChunkDetector (clause-boundary splitting)
Split long utterances at clause boundaries during interim transcripts. Each chunk translated + TTS'd independently. Language-adaptive thresholds. Context-aware translation.

**Problem:** Custom heuristics. Marker-based detection fails when host rambles without clause markers. Force-split at max_duration (2s) can cut mid-word. SOV languages (JA/KO) need special handling. Pipeline overhead (~400ms/chunk) accumulates and causes audio-video drift.

### v16 solution: Soniox v4 semantic endpointing
Soniox's endpointing classifier understands grammar completion, not just silence. It knows that a JA sentence without a verb is incomplete. It detects conversational finality semantically.

**Advantages over v15:**
- No custom marker tables or timing heuristics
- Grammar-aware splitting for SOV languages (JA 8.7% WER vs Gladia ~12-15%)
- Built-in streaming translation in same WebSocket (eliminates Google Translate hop)
- Configurable `max_endpoint_delay_ms` replaces our `min_duration`/`max_duration` system

## Pipeline Resilience (v15.1, shipped 2026-04-05)

### Fixes shipped

1. **Translation timeout** (5s) — prevents infinite pipeline hang from Google API
2. **TTS `finish()` guarantee** — unblocks audio drain on TTS error/timeout
3. **Audio queue staleness eviction** (>6s) — prevents unbounded A/V desync
4. **Queue depth limit** (10) — prevents memory growth during continuous speech
5. **Audio-video drift tracking** — `Arc<AtomicU64>` per stream, `max_drift_ms()` API
6. **Translation retry** on 5xx with 500ms backoff
7. **TTS REST fallback** for chunked pipeline (was only in legacy path)
8. **STT disconnect notification** to frontend via PipelineWarning
9. **Force-split at word boundary** instead of character boundary
10. **PipelineHealth periodic broadcast** every 5s (queue depths)
11. **Frontend pipeline health badge** + auto-reconnect (3 attempts)
12. **Chunk channel capacity** increased from 6 to 12

## Strategic Pivot — Build vs Buy the Pipeline

Evaluated 2026-04-05. The translation pipeline is a commodity input, not the moat.

### Winner: Soniox v4 Real-Time

| Dimension | Current (Gladia+Google) | Soniox v4 |
|---|---|---|
| STT+Translation latency | ~400-700ms | ~300-500ms |
| Endpointing | Silence-based + custom chunker | Semantic (grammar-aware) |
| JA WER | ~12-15% | 8.7% |
| KO WER | ~8-12% | 4.3% |
| Voice cloning | ElevenLabs (keep) | ElevenLabs (keep) |
| Cost (STT+Translation) | ~$168/mo | ~$19/mo |
| API | Gladia WS + Google REST | Single WebSocket |
| Integration effort | — | Medium (same WS architecture) |

### Eliminated alternatives

- **Qwen3-LiveTranslate:** No voice cloning (8 preset voices only). 3s latency = no improvement. Python SDK only.
- **OpenAI Realtime API:** Turn-based (incompatible with continuous broadcast). $3,200-9,800/mo.
- **Google DeepMind S2ST:** Not available as developer API. No CJK support.
- **Palabra/Pinch:** $35,000/mo for 4 languages. Not viable.

## Constraints

- Desktop app (Tauri + Rust), runs locally, no cloud GPU
- ElevenLabs Flash v2.5 for TTS (current — may be replaced by end-to-end API when available)
- Must maintain voice cloning quality (host's voice, not generic TTS voice)
- Target: **<2 second average end-to-end latency** (speech → translated audio on stream)
- Must handle 4+ languages simultaneously
- Default broadcast delay: 3000ms
