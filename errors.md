# Complete Failure Scenario Map

Status key: ✅ Fixed (v15.1) | 🔄 Fixed by Soniox migration (v16) | ⬚ Open

Pipeline Overview:

```
v15 (current):   Host Audio → [STT] → [Chunking] → [Translation] → [TTS] → [StreamingPcm] → [Audio Drain] → [FFmpeg] → [RTMP]
v16 (Soniox):    Host Audio → [Soniox STT+Translation] ─────────→ [TTS] → [StreamingPcm] → [Audio Drain] → [FFmpeg] → [RTMP]
```

---

## Stage 1: STT

### Gladia-specific (eliminated by Soniox migration)

| # | Scenario | Status | Notes |
|---|----------|--------|-------|
| 1.1 | Network drop to Gladia | 🔄 | Reconnect loop preserved for Soniox. Speech during reconnect still lost. |
| 1.2 | Gladia returns error | 🔄 | Replaced by Soniox error handling. |
| 1.3 | Gladia WebSocket read error | 🔄 | Same reconnect pattern applies to Soniox. |
| 1.4 | Gladia session POST fails | 🔄 | Soniox has simpler connection (direct WS, no session POST). |
| 1.5 | Gladia revises interim transcript aggressively | 🔄 | Soniox token protocol handles differently. No derive_position needed. |
| 1.7 | max_duration_without_endpointing hit | 🔄 | Soniox semantic endpointing replaces this. |
| 1.8 | STT API key expires mid-stream | ✅ | PipelineWarning "stt_disconnected" now sent to frontend. |
| 1.9 | Adaptive reconnect triggers | 🔄 | Not needed with Soniox — semantic endpointing adapts natively. |

### Still relevant after Soniox

| # | Scenario | Status | Notes |
|---|----------|--------|-------|
| 1.6 | STT sends no final (host keeps talking) | ⬚ | Soniox semantic endpointing should handle this, but need to verify max_endpoint_delay_ms behavior. |
| 1.10 | Packet loss / degraded connection | ⬚ | Audio quality gating before STT not implemented. |
| 1.11 | STT hallucination (transcribes non-speech) | ⬚ | No confidence score filtering. |

---

## Stage 2: Progressive Chunk Detection (eliminated by Soniox)

| # | Scenario | Status |
|---|----------|--------|
| 2.1 | Host rambles without clause markers | 🔄 Soniox semantic endpointing handles this natively. |
| 2.2 | Very short utterance falls to legacy path | 🔄 Legacy vs chunked path distinction goes away. |
| 2.3 | Rapid-fire short phrases | 🔄 Soniox manages endpointing. Still need to handle ElevenLabs concurrent limits. |
| 2.4 | Chunk channel full (capacity=6) | ✅ Increased to 12. 🔄 Channel may be removed entirely with Soniox. |
| 2.5 | Force-split cuts mid-word | ✅ Splits at word boundary now. 🔄 Eliminated with Soniox. |
| 2.6 | Host code-switches languages | 🔄 Soniox supports code switching natively. |
| 2.7 | Host makes verbal correction | ⬚ | Still unhandled. Soniox transcribes both. |
| 2.8 | Non-speech sounds (coughs, laughs) | ⬚ | No audio classifier. |
| 2.9 | Background music / audience noise | ⬚ | No server-side noise gate. |

---

## Stage 3: Translation

### Google Translate-specific (eliminated by Soniox)

| # | Scenario | Status |
|---|----------|--------|
| 3.1 | No timeout on translation request | ✅ Fixed (5s timeout). 🔄 Eliminated — translation is in Soniox WS. |
| 3.2 | Google rate limit (429) | ✅ Retry on 5xx. 🔄 Eliminated. |
| 3.3 | Google API key invalid/expired | 🔄 Eliminated. Soniox API key covers both STT+Translation. |
| 3.4 | Google API intermittent errors (500, 503) | ✅ Retry with backoff. 🔄 Eliminated. |
| 3.5 | Context separator \|\|\| mangled | 🔄 Eliminated. Soniox handles context internally. |
| 3.9 | Translation returns wrong target language | 🔄 Eliminated. |

### Still relevant after Soniox

| # | Scenario | Status | Notes |
|---|----------|--------|-------|
| 3.6 | Translation returns empty string | ⬚ | Need empty translation guard before TTS. |
| 3.7 | Translation much longer than source | ⬚ | Audio overrun still possible. Staleness eviction helps. |
| 3.8 | All translations for an utterance fail | ✅ | finish() called properly. StreamingPcm won't block forever. |

---

## Stage 4: TTS (ElevenLabs WebSocket) — unchanged by Soniox

| # | Scenario | Status | Notes |
|---|----------|--------|-------|
| 4.1 | TTS timeout | ✅ | finish() now called on timeout. |
| 4.2 | ElevenLabs rate limit / concurrent limit | ⬚ | No retry. 4 langs = 4 concurrent WS connections. |
| 4.3 | ElevenLabs returns 0 audio bytes | ⬚ | Warns but plays silence gap. |
| 4.4 | Voice clone ID deleted/invalid | ⬚ | Silent fallback to default voice. |
| 4.5 | IncrementalMp3Decoder fails | ⬚ | Partial audio plays. |
| 4.6 | ElevenLabs WS closes unexpectedly | ✅ | REST fallback now available in chunked pipeline. |
| 4.7 | 4+ concurrent TTS connections | ⬚ | Plan limits may cause failures. |
| 4.8 | TTS audio longer than utterance | ⬚ | append_with_limit truncates. |
| 4.9 | Chunked pipeline has no REST fallback | ✅ | REST fallback added. |

---

## Stage 5: StreamingPcm + Audio Queue — unchanged by Soniox

| # | Scenario | Status | Notes |
|---|----------|--------|-------|
| 5.1 | StreamingPcm never marked complete | ✅ | finish() called on all error/timeout paths. |
| 5.2 | Audio queue depth grows unbounded | ✅ | Max depth 10 + staleness eviction (>6s). |
| 5.3 | Stale play_at timestamps | ✅ | Staleness eviction drops items >6s old. |
| 5.4 | Chunk 0 fails, chunk 1+ succeeds | ⬚ | StreamingPcm may not be created. |
| 5.5 | No stream found for language | ⬚ | Orphan buffer. Wasted compute. |

---

## Stage 6: Audio Drain Thread — unchanged by Soniox

| # | Scenario | Status | Notes |
|---|----------|--------|-------|
| 6.1 | FIFO write error | ⬚ | No signal back to pipeline. |
| 6.2 | Jitter > 500ms | ✅ | Recovery mode with catch-up (capped at 2s). |
| 6.3 | Drift accumulates over long session | ✅ | Drift tracked per stream via AtomicU64. |
| 6.4 | Extended silence (no audio queued) | ⬚ | Core problem. Soniox should reduce this by cutting latency ~1s. |
| 6.5 | Audio-video desync with no catch-up | ⬚ | Skip-ahead logic (B3) not yet implemented. |

---

## Stage 7-8: Video Drain + FFmpeg + RTMP — unchanged by Soniox

| # | Scenario | Status |
|---|----------|--------|
| 7.1 | FFmpeg crash | ✅ Health monitor + restart (max 50). |
| 7.2 | Video buffer overflow | ✅ Drops oldest chunks (>600). |
| 7.3-7.5 | RTMP issues | ⬚ Restart loop, no auth failure detection. |
| 8.1-8.6 | Platform-specific | ⬚ No per-language status notification. |

---

## Stage 9-10: Frontend — partially addressed

| # | Scenario | Status | Notes |
|---|----------|--------|-------|
| 9.1 | Host WebSocket disconnect | ⬚ | In-flight tasks not cancelled. |
| 9.2 | Frontend doesn't receive messages | ⬚ | Silent drop if WS buffer full. |
| 9.3 | Client reconnects after disconnect | ✅ | Auto-reconnect (3 attempts, 2s delay). |
| 10.1 | No auto-reconnect | ✅ | Implemented. |
| 10.4 | No pipeline health visibility | ✅ | PipelineHealthBadge + PipelineHealth every 5s. |
| 10.5 | Error deduplication | ⬚ | Not implemented. |
| 10.6 | No graceful degradation | ⬚ | No subtitle-only fallback. |

---

## Stage 11-12: Resource Exhaustion + Session Lifecycle — unchanged

All items (11.1-11.6, 12.1-12.5) remain ⬚ Open. These are longer-term reliability concerns for marathon streams (8+ hours).

---

## The 7 Critical Gaps — Status

1. ~~Translation has no timeout~~ → ✅ Fixed (5s timeout)
2. ~~TTS timeout doesn't call finish()~~ → ✅ Fixed
3. ~~No audio queue depth limit or staleness eviction~~ → ✅ Fixed (depth 10, >6s eviction)
4. ~~No drift detection between audio and video~~ → ✅ Fixed (AtomicU64 per stream)
5. **No backpressure from audio to STT** → ⬚ Open (less critical with Soniox semantic endpointing)
6. **No graceful degradation** → ⬚ Open
7. ~~Legacy pipeline has no streaming TTS~~ → Partially addressed (REST fallback added). 🔄 Legacy vs chunked distinction may go away with Soniox.
