# Host View Benchmark Dashboard — Implementation Spec

Everything goes on the Host page. Two sections: (1) live latency tracking per utterance, (2) pipeline comparison and decision rationale. The host view becomes the full story.

---

## Part 1: Live Latency Dashboard

Real-time per-utterance timing as the host speaks.

### Data Source

Host WebSocket already receives timing data:

- `{ type: "final", utteranceId }` → record `Date.now()` as pipeline start
- `{ type: "translation", utteranceId, translateMs }` → NLLB timing (server-side)
- `{ type: "tts_end", utteranceId, ttsMs }` → Kokoro timing (server-side)

Compute on frontend:

- `translateMs` — from translation message
- `ttsMs` — from tts_end message
- `totalMs` — Date.now() at tts_end minus Date.now() at final (wall-clock browser-side)
- `overheadMs` — totalMs - translateMs - ttsMs (network + worker routing)

### State (add to useHostRoom hook)

```typescript
interface UtteranceTiming {
  id: string;
  text: string; // truncated to 40 chars
  translateMs: number;
  ttsMs: number;
  totalMs: number;
  overheadMs: number;
  timestamp: number;
  langs: string[]; // which language groups were active
}

const [timings, setTimings] = useState<UtteranceTiming[]>([]);
const finalTimestamps = useRef<Map<string, number>>(new Map());

// On "final" message: record start timestamp
finalTimestamps.current.set(utteranceId, Date.now());

// On "tts_end" message: compute total, push to timings
const start = finalTimestamps.current.get(utteranceId);
if (start) {
  const totalMs = Date.now() - start;
  setTimings((prev) => [
    ...prev,
    {
      id: utteranceId,
      text: lastTranscript.slice(0, 40),
      translateMs,
      ttsMs,
      totalMs,
      overheadMs: Math.max(0, totalMs - translateMs - ttsMs),
      timestamp: Date.now(),
      langs: Object.keys(guestCounts).filter((l) => guestCounts[l] > 0),
    },
  ]);
}
```

Return `timings` from the hook so HostPage can render it.

### UI — Live Timing Panel

Render below the mic/transcript area on HostPage. Always visible (not collapsible).

#### Summary Stats Bar (top)

```
⚡ Avg: 400ms  │  Best: 221ms  │  Target: 300ms  │  Gap: 1.3x  │  n=6
```

Single row, monospace numbers. Updates after each utterance.

#### Per-Utterance Stacked Bars

Each utterance gets a row with a horizontal stacked bar:

```
#5  "How are you today"                                    1,842ms
    ████████ translate 562ms  ██████████████████ tts 1,104ms  ██ 176ms
    ┊← 300ms

#4  "Hello everyone"                                       1,214ms
    ██████ translate 394ms  ████████████ tts 787ms  █ 33ms
    ┊← 300ms

#3  "Welcome to our show"                                  2,310ms
    ████████████ translate 870ms  ████████████████████ tts 1,369ms  ██ 71ms
    ┊← 300ms
```

Colors:

- **Purple (#8b5cf6)** — Translation (NLLB)
- **Amber (#f59e0b)** — TTS (Kokoro)
- **Gray (#666)** — Overhead (network/routing)
- **Green dashed line** — 300ms target marker (always visible as a thin vertical line)

Bar scaling: all bars share the same scale. Max = longest utterance in session. The 300ms target line stays at a fixed position relative to this scale, so you can visually see how far each utterance is from the target.

Newest utterances appear at top. Show last 10 max, scroll if more.

---

## Part 2: Pipeline Comparison (Static Section)

Below the live timing panel, add a "Pipeline Analysis" section. This is static (hardcoded data from measurements). It tells the story of WHY each component was chosen.

### Section: My Pipeline vs Brivva's Listed Pipeline

Two columns or two stacked cards:

**My Prototype (v3 — fully self-hosted):**
| Phase | Tech | Measured |
|-------|------|---------|
| STT | CF Nova-3 (via stt-wrapper) | streaming ✓ |
| Translation | NLLB (self-hosted GPU) | 75–337ms |
| TTS | Kokoro 82M (GPU) | 111–358ms |
| Server | Rust axum + DashMap | fan-out per language |
| Lip-sync | — | not built |

**Brivva's Listed Pipeline:**
| Phase | Tech | Status |
|-------|------|--------|
| STT | Whisper | batch only |
| Translation | Context NMT | unknown latency |
| TTS | Emotive TTS (3B) | unknown latency |
| Lip-sync | Wav2Lip | lips only |
| Target | — | <300ms e2e |

### Section: Component Decisions (collapsible, default collapsed)

Three expandable sections. Each has a comparison table + one-line verdict.

**STT — Why CF Nova-3 (via stt-wrapper):**
| Model | WER | Streaming | Price |
|-------|-----|-----------|-------|
| ✓ CF Nova-3 (stt-wrapper) | 6.5% | Yes — streaming | CF AI Gateway |
| WhisperLiveKit (base.en) | — | Yes — live interims | Free (self-hosted) |
| Whisper v3 Turbo | 4.8% | No — batch only | $0.67/1000min |
Verdict: "Streaming is non-negotiable for live translation. Self-hosted eliminates API costs and latency to cloud."

**Translation — Why NLLB (self-hosted):**
| Model | Type | Latency |
|-------|------|---------|
| ✓ NLLB (self-hosted GPU) | Seq2Seq | 75–337ms |
| M2M100-1.2B (CF Workers AI) | Seq2Seq | 394–870ms |
| Llama 3.2 1B | LLM prompted | ~1,500ms |
Verdict: "Self-hosted NLLB removes CF dependency. Same seq2seq architecture, no network roundtrip to edge."

**TTS — Why Kokoro (and its limits):**
| Model | Params | Latency | Expressive |
|-------|--------|---------|------------|
| ✓ Kokoro (self-hosted GPU) | 82M | 111–358ms | Limited |
| Kokoro (Replicate) | 82M | 2,000–14,000ms | Limited |
| Emotive TTS (Brivva) | 3B | unknown | Yes — emotions |
Verdict: "Kokoro wins for demo speed. Production needs expressive TTS for live commerce energy."

### Section: Gap to 300ms

```
Translation:  ████████░░  172ms → 50ms target (3x)
TTS:          ██████████░░  202ms → 100ms target (2x)
Overhead:     ██░░  25ms → 20ms target (1.3x)
─────────────────────────────────────────────────────
Total:        ~400ms → 300ms (1.3x gap)
```

### Section: Optimization Roadmap (table)

| Optimization                    | Savings               | Status            |
| ------------------------------- | --------------------- | ----------------- |
| Self-hosted pipeline (Rust)     | −1700ms avg           | Done ✓            |
| STT wrapper (clean events)      | −complexity           | Done ✓            |
| Parallel translations           | —                     | Done ✓            |
| GPU inference (NLLB + Kokoro)   | −5x latency           | Done ✓            |
| Streaming TTS playback          | −100ms perceived      | Medium            |
| Co-locate all models on one GPU | −20ms network         | Brivva infra      |
| End-to-end speech-to-speech     | paradigm shift        | Brivva's R&D goal |

One-line footer: "True 300ms needs co-located models or end-to-end S2S. That's the R&D challenge."

---

## Implementation

### Files to Create/Modify:

1. **`useHostRoom.ts`** — add `UtteranceTiming[]` state, timestamp tracking on final/tts_end
2. **`HostPage.tsx`** — render latency dashboard + pipeline comparison below existing UI
3. Optionally extract into components:
   - `LatencyDashboard.tsx` — live timing panel
   - `PipelineAnalysis.tsx` — static comparison section

### Style

- Dark theme, match existing app style
- Monospace for all numbers
- Green (#4ade80) = target/good, Amber (#fbbf24) = medium, Red (#f87171) = slow, Purple (#8b5cf6) = translation, Gray (#666) = overhead
- Compact — live dashboard shouldn't be larger than the transcript area
- Pipeline comparison sections are collapsible to keep the view clean when just demoing
- Mobile-friendly but optimized for desktop (this will be shown on a laptop during the interview)

### Test

1. Create room as host, have guest join in another tab
2. Speak 5+ utterances
3. Verify: stacked bars appear after each utterance, running stats update
4. Verify: 300ms target line is visible on every bar
5. Scroll down to see static pipeline comparison
6. Deploy: `npm run deploy` in frontend/
