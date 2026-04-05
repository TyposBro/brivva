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

## Priority 2: Fix Critical Bugs (Current Focus)

### P0 — FFmpeg crash on first video chunk (BLOCKING)

**Every session:** FFmpeg crashes on the first video data with `could not find corresponding trex (id 1)` → exit 183. Auto-restarts (attempt 1/50) but loses ~5-7s of video. This causes a permanent ~5s audio-ahead-of-video desync because audio keeps flowing during the crash+restart cycle.

**Root cause:** MediaRecorder outputs fragmented MP4. The init segment (moov/trex) must be written to FFmpeg's stdin before the first moof fragment. The video drain is writing data chunks before FFmpeg has parsed the init segment headers.

**Evidence:**
- Happens in every test session (3/3 on Apr 5-6)
- `[VIDEO] replaying init segment (929431B) for restart` — init segment IS available, but only replayed on restart, not on first spawn
- After restart, FFmpeg works perfectly — the restart correctly writes init segment first
- Audio-video desync confirmed: at 1s broadcast delay, audio arrives ~5s before video (tested with finger counting)

| # | Task | Status | Notes |
|---|------|--------|-------|
| **V1** | **Fix init segment ordering** | **Open** | Ensure init segment is written to FFmpeg stdin before any moof data. Likely need to buffer video chunks until init segment is captured, then write init + buffered chunks. |
| V2 | TTS cold-start empty response | Open | First 1-2 ElevenLabs TTS calls return 0 audio bytes. Likely WebSocket not fully ready. Consider warm-up call or retry. |
| V3 | TTS timeout at 1s broadcast delay | Open | Multiple TTS timeouts when delay=1s. TTS deadline is min(broadcast_delay-500ms, 10s) — at 1s delay, deadline is 500ms. Too short for Expressive model. |

### Production Polish

Quality and reliability for the demo. Target: 30+ min session, zero hiccups.

| # | Task | Status | Notes |
|---|------|--------|-------|
| P1 | E2E live test (KO→JP) | Open | Full test on Coupang KR → Rakuten JP with real merchant accounts. Blocked by V1. |
| P2 | Voice cloning quality test | Open | 30s sample → cloned voice → compare with real host. Is it "better than hiring a human"? |
| P3 | 30-minute endurance test | Open | Continuous streaming, monitor for crashes, drift, FIFO starvation. Blocked by V1. |
| P4 | Backup demo recording | Open | Screen record a flawless session in case live demo fails. Blocked by V1. |
| P5 | Test Expressive TTS model | Open | Video lags audio by ~5s → headroom for slower but higher-quality TTS. Blocked by V1 (need accurate sync first). |

### Resilience (from v15.1 analysis, still open)

| # | Task | Status | Notes |
|---|------|--------|-------|
| A3 | Empty translation guard | Open | Check `translated.trim().is_empty()` before TTS. Prevents empty TTS calls. |
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
