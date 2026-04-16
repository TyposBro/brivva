# Brivva System Design — Holes, Alternatives & Action Plan

**Date:** Apr 14, 2026 | **Demo:** Apr 16, 1-4pm | **Status:** Merged final report

## Core Insight

Soniox owns translation segmentation boundaries. Local pipeline pretends it controls them. This is the central design contradiction.

The fix is not smarter chunking. It is:
1. A lightweight parallel translation path for when Soniox accumulates too long
2. An automatic degradation policy for when the pipeline leaves the safe operating envelope
3. Concrete thresholds, not vague policies

## Current Pipeline Strengths (Keep These)

- Simple N+1 connection model with audio fanout
- A/V sync, RTMP muxer, jitter buffer, crash recovery — the real moat
- Source-language passthrough at zero API cost
- Provider-swappable design (proved by Gladia→Soniox migration)

---

## Hole 1: Translation Accumulation During Rapid Speech

**Severity:** High — direct demo risk
**When:** Pre-demo

### Problem

Soniox accumulates translation for continuous speech, emits only at semantic endpoint. Live commerce hosts do 15-20s rapid-fire product pitches without pausing. Current mitigations:

- Force-chunk fires on transcript duration but `translation_acc` is empty → resets timer, no TTS
- Speech-duration pacing pads silence *after* TTS, doesn't prevent the initial 15s gap
- Staleness eviction drops audio entirely

**Result:** 15s silence → burst of translated audio → A/V desynced. Exactly during the content that matters most.

### Solution: Lightweight Parallel Translation

Not a full segmentation rewrite. ~100 lines of code:

- Soniox remains primary STT + translation for natural utterances
- When force-chunk watchdog fires at 4s AND `translation_acc` is empty:
  - Send accumulated source transcript to DeepL API (single REST call)
  - Use DeepL translation for TTS immediately
  - If Soniox semantic endpoint arrives later with better translation, use for next context window
- When Soniox endpoint fires naturally (<4s), use Soniox translation as before (zero change)

| Aspect | Detail |
|--------|--------|
| **Trigger** | Force-chunk at 4s + empty `translation_acc` |
| **API** | DeepL Free/Pro — 500k chars/mo free, $5.49/mo pro |
| **Latency** | DeepL REST: ~200-400ms |
| **Context** | Send last 2 sentences as context prefix separated by `\n` |
| **Fallback** | If DeepL fails, continue waiting for Soniox (current behavior) |

### Alternatives Considered

| Option | Verdict |
|--------|---------|
| Full controlled segmentation (local chunking + external translation) | Rejected. Too much code, too much risk before demo |
| Accept Soniox timing as hard constraint, degrade only | Rejected. 15s silence during product pitch = demo failure. Degradation alone doesn't solve it |
| Soniox partial flush API | Unknown if exists. Vendor-dependent |
| **Hybrid: Soniox primary + DeepL fallback at 4s** | **Recommended.** Minimal code, solves exact problem |

---

## Hole 2: No Graceful Degradation

**Severity:** High — reliability
**When:** Post-demo sprint (except subtitle fix — needs investigation before demo)

### Problem

System is binary: works perfectly or drops audio via staleness eviction. No middle ground.

### Degradation Ladder

```
Mode A: Translated cloned voice (full pipeline)
    ↓ drift > 2s sustained for > 10s, OR 3+ TTS timeouts in 60s
Mode B: Translated cloned voice + reduced emotion (neutral/high-energy only)
    ↓ drift > 4s sustained for > 15s, OR 5+ TTS failures in 60s
Mode C: Source audio passthrough (wrong language, stream lives)
    ↓ FFmpeg crash unrecoverable
Mode D: Stream offline → post-processed dubbed replay (Tier 4)
```

**Note:** Original report proposed "subtitles only" as intermediate mode. Subtitle overlay (`-vf drawtext`) is currently broken — causes YouTube "Preparing stream" indefinitely (fontconfig issue in bundled FFmpeg, `problem.md` line 144-148). Subtitles cannot be a degradation step until fixed. Ladder above skips subtitles.

### Concrete Thresholds

| Parameter | Value | Rationale |
|-----------|-------|-----------|
| Downgrade trigger: drift | >2s sustained for >10s | 2s = noticeable desync. 10s = not transient |
| Downgrade trigger: TTS failures | 3+ timeouts in 60s window | Pattern, not spike |
| Downgrade trigger: queue depth | >8 items (of 10 max) | Near eviction threshold |
| Recovery trigger: drift | <500ms sustained for >30s | Must prove stability |
| Recovery trigger: TTS | 0 failures in 60s window | Clean bill of health |
| Anti-flap minimum | 60s between mode changes | Prevent oscillation |
| Mode change visibility | Log + frontend status indicator | Operator must know |

### Per-Language Fault Isolation

Each target language = independent fault domain with its own:
- Health state (enum: `Healthy`, `Degraded`, `Passthrough`, `Offline`)
- Drift tracker (already exists as `Arc<AtomicU64>`)
- TTS failure counter (sliding 60s window)
- Queue depth
- Current output mode
- Last successful translation timestamp
- Last successful TTS timestamp

One language degrading MUST NOT affect others. Japanese can be in Mode C while English stays in Mode A.

---

## Hole 3: Emotion Misclassification

**Severity:** Medium — demo risk
**When:** Pre-demo (low effort)

### Problem

9-class emotion classifier from pitch autocorrelation + energy RMS + pause density. Rule-based on noisy features. Korean prosody norms ≠ English training assumptions. Misclassification → TTS voice sounds wrong (excited when calm, somber when energetic). Customers judge voice quality most harshly.

### Solution: Reduce to 2 States for Demo

| State | Trigger | ElevenLabs Mapping |
|-------|---------|-------------------|
| **Neutral** | RMS energy < threshold | `stability: 0.7, style: 0.0` |
| **High-energy** | RMS energy ≥ threshold | `stability: 0.5, style: 0.3` |

- Single threshold on RMS energy — language-agnostic, no pitch analysis needed
- Threshold calibrated from first 10s of session audio (running average)
- No pause density analysis, no autocorrelation, no 9-class classifier

### Post-Demo Path

Add calibration phase: first 30s of session establishes per-host prosody baseline. Relative thresholds from that baseline. Expand to 3-4 states once calibrated.

---

## Hole 4: TTS Result Caching

**Severity:** Medium — latency + cost
**When:** Post-demo sprint

### Problem

Live commerce hosts repeat: product names, prices, "limited time only", CTAs. Each repetition = full translate→TTS round-trip (~300-500ms + API cost). Same text + same voice + same emotion = same audio.

### Solution: LRU TTS Cache

```
Key:   (translated_text, voice_id, emotion_state)  // emotion_state = neutral|high_energy
Value: Vec<u8> (PCM audio bytes)
TTL:   session lifetime
Max:   200 entries (~50MB at avg 5s utterances)
```

- Cache hit → skip TTS, queue PCM directly (save 300ms+)
- Exact string match only — no fuzzy matching (complexity not worth it)
- Cache is per-session, cleared on session end
- ~50 lines of code: `HashMap` with LRU eviction

### Future: Phrase Pre-Warming

Host uploads product script before session → pre-translate + pre-generate TTS for key phrases. Cache pre-populated. Zero latency on scripted content. Demo differentiator: "load your product list, we pre-translate."

---

## Hole 5: Static Broadcast Delay

**Severity:** Medium — latency optimization
**When:** Post-demo

### Problem

Fixed 3s delay for all languages. Pipeline latency varies by language pair and TTS provider. EN→JA may need 4s, KO→EN may only need 2s. Static delay = suboptimal for all.

### Solution: Per-Language Delay

| Config | Value | Rationale |
|--------|-------|-----------|
| Source passthrough | 0ms (or minimal) | No processing, no delay needed |
| ElevenLabs Flash target | 3000ms | Current default, proven |
| ElevenLabs Turbo target | 4000ms | ~200ms higher TTFB |
| DashScope target | 5000ms | Already configured |
| Per-language override | Configurable in session settings | Operator tuning |

Post-demo: adaptive delay that tracks P95 pipeline latency per language with 500ms margin. Auto-adjusts every 60s. Cap at 7s.

---

## Hole 6: N+1 Connection Scaling

**Severity:** Low — future concern
**When:** When expanding to 5+ languages

### Problem

4 languages = 5 Soniox WebSockets receiving identical audio. Linear bandwidth scaling. Independent reconnection per connection.

### Solution Path (Not Now)

At 8+ languages, switch to: 1 Soniox connection (source transcript only) + fan out transcript to DeepL/Google for N translations. Decouples STT from translation. STT cost fixed regardless of language count.

Not worth changing for 4 languages. Current model works.

---

## Demo-Specific Checklist

### Go Criteria

- [ ] 30-minute endurance run without crash
- [ ] No visible sustained A/V sync failure (>2s offset for >10s)
- [ ] No repeated TTS failure loops
- [ ] No queue runaway in any target language
- [ ] Parallel translation fallback works for rapid speech (>4s continuous)
- [ ] Emotion reduced to 2 states, no embarrassing voice mismatches
- [ ] Operator can explain what happens if something degrades
- [ ] Product demonstration sequences (holding items while narrating) show <2s audio-action offset

### No-Go Criteria

- [ ] Translated voice continues playing after drift is obviously unsafe (>4s)
- [ ] System piles up multiple stale utterances audibly
- [ ] One unhealthy language destabilizes other language streams
- [ ] Voice sounds robotic or has wrong emotional tone during selling segments
- [ ] System hides failure instead of degrading visibly

---

## Implementation Priority

| # | What | Risk | Effort | When | Lines of Code |
|---|------|------|--------|------|---------------|
| 1 | Parallel translation (DeepL fallback at 4s) | Stream quality | ~100 LOC | **Pre-demo** | ~100 |
| 2 | Emotion → 2 states | Voice quality | ~30 LOC change | **Pre-demo** | ~30 (net reduction) |
| 3 | Degradation ladder | Reliability | ~300 LOC | Post-demo sprint | ~300 |
| 4 | Per-language fault isolation | Reliability | ~200 LOC | Post-demo sprint | ~200 |
| 5 | TTS result cache | Latency + cost | ~50 LOC | Post-demo sprint | ~50 |
| 6 | Per-language delay | Latency | ~80 LOC | Post-demo | ~80 |
| 7 | Subtitle fix (drawtext/fontconfig) | Degradation ladder needs it | Unknown | Post-demo | Unknown |
| 8 | N+1 → 1+N architecture | Scale | Medium | 5+ languages | ~500 |

---

## Bottom Line

The biggest hole is not algorithmic sophistication. It is two things:

1. **No fallback translation** when Soniox accumulates too long — solvable with ~100 lines of DeepL integration
2. **No degradation policy** when pipeline leaves safe envelope — solvable with explicit state machine and concrete thresholds

For the demo: ship #1 (parallel translation) and #2 (emotion reduction). These directly address the two highest-risk failure modes: silent gaps during product pitches and weird voice during selling.

Everything else ships in the post-demo sprint.
