# Pipeline TODO

## Priority 1: Soniox v4 Migration (v16) — COMPLETE

All tasks done. Gladia, Google Translate, ProgressiveChunkDetector fully removed.

- [x] S1. Soniox connection module (`wss://stt-rt.soniox.com/transcribe-websocket`, model `stt-rt-v4`)
- [x] S2. Soniox message handler (token accumulation, semantic endpointing, translation extraction)
- [x] S3. Wire translation from Soniox response (native `translation_status` field, no external API)
- [x] S4. Remove dead code (detectors.rs, markers.rs, Google Translate, derive_position — ~595 lines deleted)
- [x] S5. Update config (Soniox API key, `max_endpoint_delay_ms: 1500`, N+1 connection model)
- [x] S6. End-to-end implementation (N+1 connections, audio fanout, force-chunk at 4s, source passthrough)

---

## Priority 2: Critical Bug Fixes — COMPLETE (Apr 6)

| # | Bug | Fix | Status |
|---|-----|-----|--------|
| **V1** | FFmpeg crash on first video chunk (P0) | `video_drain.rs` — `write_init_segment_on_first_spawn()` writes init segment before data chunks. | **Fixed** |
| V2 | TTS 0 bytes on first calls | `ws.rs` — retry once, then REST fallback on second 0-byte response. | **Fixed** |
| V3 | TTS timeout at low broadcast delay | `pipeline_budget.rs` — `TTS_DEADLINE_FLOOR_MS = 3000`. | **Fixed** |
| V4 | Force-chunk stall (24s+ accumulation) | `handler.rs` — fires on transcript duration, not translation presence. | **Fixed** |
| V5 | Audio pile-up on stale TTS | `manager.rs` — `cap_stale_play_at()` caps when `utterance_start` > `delay + 2s`. | **Fixed** |
| V6 | Audio outrunning video on asymmetric lang pairs | `audio_drain.rs` — speech-duration pacing pads silence after short TTS. | **Fixed** |

### Production Polish (Current Focus)

Quality and reliability for the demo. Target: 30+ min session, zero hiccups.

| # | Task | Status | Notes |
|---|------|--------|-------|
| P1 | E2E live test (KO→JP) | Open | Full test on Coupang KR → Rakuten JP with real merchant accounts |
| P2 | Voice cloning quality test | Open | 30s sample → cloned voice → compare with real host. Is it "better than hiring a human"? |
| P3 | 30-minute endurance test | Open | Continuous streaming, monitor for crashes, drift, FIFO starvation |
| P4 | Backup demo recording | Open | Screen record a flawless session in case live demo fails |
| P5 | Test Expressive TTS model | Open | Slower but higher-quality TTS. Need accurate sync first. |

### Resilience (from v15.1 analysis, still open)

| # | Task | Status | Notes |
|---|------|--------|-------|
| B3 | Skip-ahead logic | Open | When drift > 1.5x broadcast_delay, skip to newest complete utterance. |
| C1 | Pipeline failure counters | Open | Replace hardcoded 0s in pipeline_health.rs with real AtomicU64 counters. |
| C2 | E2E latency tracking | Open | Record chunk_start → TTS_complete, rolling average. |
| C3 | Circuit breaker | Open | Generic circuit breaker for TTS + Soniox APIs. |
| C4 | Wire health reporter | Open | Depends on C1. Replace 0s with real data. |

### Frontend Polish

| # | Task | Status | Notes |
|---|------|--------|-------|
| D1 | Full health dashboard | Open | Consume PipelineHealth in frontend. Per-language queue/drift display. |
| D2 | Per-language stream status | Open | Green/yellow/red dot per RTMP target. |
| D3 | Graceful degradation | Open | Auto subtitle-only when drift > threshold. |
| D4 | Error deduplication | Open | Deduplicate identical errors within 5s window. |

---

## Priority 3: Future (Post-Demo)

| # | Task | Notes |
|---|------|-------|
| F1 | Lipsync (Tiers 3-4) | Real-time and post-processed. Simon estimates 6mo-1yr for tech maturity. |
| F2 | macOS code signing | Apple Developer account ($99/yr) for distribution outside dev machine. |
| F3 | Windows build | Tauri supports Windows. FFmpeg sidecar needs Windows binary. |
| F4 | LongCat-AudioDiT for Option 4 | Open-source SOTA voice cloning for post-processed (non-live) content. Needs GPU. |
| F5 | Monitor Google Cloud S2ST API | When available, could replace entire STT→Translate→TTS cascade. No CJK yet. |

---

## Completed

### v16.1 — Bug Fixes & A/V Sync (Apr 6, 2026)
- [x] V1. FFmpeg init segment on first spawn (was crashing every session)
- [x] V2. TTS cold-start retry + REST fallback on 0 bytes
- [x] V3. TTS deadline floor (3s minimum, decoupled from broadcast delay)
- [x] V4. Force-chunk fires on transcript duration (Soniox withholds translation tokens during continuous speech)
- [x] V5. Stale play_at capping (prevents audio pile-up when TTS is slow)
- [x] V6. Speech-duration pacing (prevents audio outrunning video on asymmetric language pairs)

### v16 — Soniox Migration (Apr 5, 2026)
- [x] Full Soniox v4 integration (N+1 connections, semantic endpointing, native translation)
- [x] Gladia removal (zero references)
- [x] Google Translate removal (zero references)
- [x] ProgressiveChunkDetector removal (~595 lines deleted)
- [x] 4-second force-chunk threshold
- [x] Source-language passthrough
- [x] Prosody → emotion → voice style pipeline

### v15.1 — Resilience (Apr 5, 2026)
- [x] A1. Translation timeout (5s)
- [x] A2. TTS finish() guarantee
- [x] A4. Translation retry with 500ms backoff on 5xx
- [x] A5. TTS REST fallback for chunked pipeline
- [x] B1. Audio queue staleness eviction (>6s, max depth 10)
- [x] B2. Audio-video drift detector (Arc<AtomicU64>, max_drift_ms())
- [x] C4. Pipeline health reporter (partial — queue depths, counters hardcoded 0)
- [x] D1. Pipeline health badge (partial — warning count)
- [x] D5. WebSocket auto-reconnect (3 attempts, 2s delay)
- [x] Force-split at word boundary
- [x] STT disconnect notification (PipelineWarning)

### v15 — Progressive Chunking (Apr 4, 2026) — superseded by Soniox
- [x] IncrementalMp3Decoder (streaming MP3→PCM)
- [x] ProgressiveChunkDetector (clause-boundary splitting) — now deleted
- [x] Chunked pipeline orchestration — now simplified
- [x] Default broadcast delay 5000ms → 3000ms
