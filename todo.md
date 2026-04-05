# Pipeline TODO

## Priority 1: Soniox v4 Migration (v16)

Replace Gladia STT + ProgressiveChunkDetector + Google Translate with Soniox v4 Real-Time.
ElevenLabs TTS, StreamingPcm, audio drain, FFmpeg, RTMP — all unchanged.

```
Workstream S ────────────────────────────────────────────────────────
(Soniox Migration)

S1. Soniox connection module     (replace shared/stt/connection.rs)
S2. Soniox message handler       (replace shared/stt/handler.rs + interim_handler + final_handler)
S3. Wire translation from Soniox (replace shared/translation/)
S4. Remove dead code             (detectors.rs, markers.rs, Google Translate, derive_position)
S5. Update config + adaptive     (replace Gladia-specific config with Soniox params)
S6. Integration test             (end-to-end: audio in → translated text out)
```

### S1. Soniox connection module

- **File:** Replace `server-rs/src/shared/stt/connection.rs`
- **What:** Connect to Soniox v4 WebSocket (`wss://stt-rt.soniox.com/transcribe-websocket`). Send config JSON with: `model: "stt-rt-preview"`, `language`, `include_nonfinal: true`, `semantic_endpointing: true`. Reuse the existing reconnect loop structure from `reconnect.rs`.
- **API key:** `SONIOX_API_KEY` env var, read in orchestration config.
- [ ] Done

### S2. Soniox message handler

- **File:** Replace `server-rs/src/shared/stt/handler.rs`, `interim_handler.rs`, `final_handler.rs`
- **What:** Soniox sends token-by-token results with `is_final` flag per token. Accumulate tokens into transcript. On semantic endpoint (final=true on last token), emit to pipeline. Interim transcripts from non-final tokens → send to frontend.
- **Key difference from Gladia:** No separate interim/final message types. Tokens arrive continuously. Semantic endpointing determines when a "sentence" is complete.
- **Translation:** Soniox includes translated text in the same response when translation is enabled. Extract it directly — no Google Translate call needed.
- [ ] Done

### S3. Wire translation from Soniox response

- **File:** Remove `server-rs/src/shared/translation/mod.rs` (Google Translate)
- **What:** Soniox response includes `translation` field when configured with `translation_config: { target_languages: ["ja", "ko", "zh"] }`. Extract translated text per language directly from the WebSocket response. Feed to TTS pipeline.
- **Context handling:** Soniox handles cross-sentence context internally — no `|||` separator needed.
- [ ] Done

### S4. Remove dead code

- **Files to delete:**
  - `server-rs/src/shared/stt/detectors.rs` (ProgressiveChunkDetector, MarkerDetector, FallbackDetector)
  - `server-rs/src/shared/stt/markers.rs` (clause marker tables)
  - `server-rs/src/shared/translation/mod.rs` (Google Translate)
- **Files to simplify:**
  - `server-rs/src/shared/stt/state.rs` — remove chunk_detector, progressive, chunk_pipeline_tx, chunk_index fields
  - `server-rs/src/shared/stt/config.rs` — remove Gladia-specific constants, marker configs
  - `server-rs/src/core/config.rs` — remove TRANSLATE_TIMEOUT_MS, TRANSLATE_RETRY_DELAY_MS, CHUNK_PIPELINE_CAPACITY
- [ ] Done

### S5. Update config + adaptive endpointing

- **File:** `server-rs/src/orchestration/config.rs`, `server-rs/src/shared/stt/config.rs`
- **What:** Replace Gladia API key with Soniox API key. Configure `max_endpoint_delay_ms` (replaces our min/max_duration). Remove adaptive reconnect logic (Soniox handles endpointing natively). Keep STT reconnect loop for network failures.
- [ ] Done

### S6. Integration test

- **What:** Record a test audio file (10s of speech). Send through Soniox → verify translated text arrives. Verify interim updates. Verify semantic endpointing fires at clause boundaries.
- [ ] Done

---

## Priority 2: Remaining Resilience (can be done during or after Soniox migration)

### Still open from v15.1 analysis

| # | Task | Status | Notes |
|---|------|--------|-------|
| A3 | Empty translation guard | Open | Check `translated.trim().is_empty()` before TTS. Still needed with Soniox. |
| B3 | Skip-ahead logic | Open | When drift > 1.5x broadcast_delay, skip to newest complete utterance. |
| B4 | Backpressure signal | Open | `is_audio_behind()` on RtmpManager → adjust chunking aggressiveness. Less relevant with Soniox semantic endpointing. |
| C1 | Pipeline failure counters | Open | Replace hardcoded 0s in pipeline_health.rs with real AtomicU64 counters. |
| C2 | E2E latency tracking | Open | Record chunk_start → TTS_complete, rolling average. |
| C3 | Circuit breaker | Open | Generic circuit breaker for TTS + Soniox APIs. |
| C4 | Wire health reporter | Open | Depends on C1. Replace 0s with real data. |
| D1 | Full health dashboard | Open | Consume PipelineHealth in frontend. Per-language queue/drift display. |
| D2 | Per-language stream status | Open | Green/yellow/red dot per RTMP target. |
| D3 | Graceful degradation | Open | Auto subtitle-only when drift > threshold. |
| D4 | Error deduplication | Open | Deduplicate identical errors within 5s window. |

---

## Completed (v15.1, shipped 2026-04-05)

- [x] A1. Translation timeout (5s)
- [x] A2. TTS finish() guarantee
- [x] A4. Translation retry with 500ms backoff on 5xx
- [x] A5. TTS REST fallback for chunked pipeline
- [x] B1. Audio queue staleness eviction (>6s, max depth 10)
- [x] B2. Audio-video drift detector (Arc<AtomicU64>, max_drift_ms())
- [x] C4. Pipeline health reporter (partial — sends queue depths, counters hardcoded 0)
- [x] D1. Pipeline health badge (partial — warning count)
- [x] D5. WebSocket auto-reconnect (3 attempts, 2s delay)
- [x] Chunk channel capacity: 6 → 12
- [x] Force-split at word boundary
- [x] STT disconnect notification (PipelineWarning)
- [x] PipelineWarning messages for translate/TTS/chunk errors
