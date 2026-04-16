# System Design Holes & Alternatives — Revised (Apr 14, 2026)

## Executive Summary

The original report identifies several real weaknesses, but it overreaches on the highest-risk fix by proposing a second translation path before the demo.

That is the wrong tradeoff right now.

The best near-term approach is:

- keep Soniox native translation
- accept provider-owned segmentation as a hard constraint
- stop pretending local force-chunking gives true chunk control
- add explicit graceful degradation
- reduce voice-style variance
- avoid adding new timing-sensitive subsystems before the demo

The biggest missing capability is not better segmentation. It is a clear policy for when to degrade.

## 1. Translation Accumulation During Rapid Speech (High — Demo Risk)

### Hole

This is a real problem.

The current design notes that Soniox translation accumulation is "not an issue for natural commerce speech with normal pauses." That is too optimistic for live commerce. Real hosts often run long product pitches, stacked offers, price drops, urgency messaging, and feature bundles without clean pauses.

In those cases:

- transcript keeps flowing
- local force-chunk watchdog may fire
- translation payload may still be empty
- Soniox may emit a large translation burst later

That creates the worst possible viewer experience:

- long silence in translated audio
- then a burst of compressed translated speech
- semantic lag relative to the on-screen demo

### What Current Mitigations Actually Do

Current mitigations are still useful:

- force-chunk watchdog
- speech-duration pacing
- stale `play_at` capping
- queue limits

But they are compensating controls, not true fixes.

They:

- reduce pile-up
- smooth output timing
- limit runaway drift

They do **not**:

- force Soniox to emit earlier translation
- restore semantic alignment once provider timing has slipped

### Alternatives

| Option | Pros | Cons |
|--------|------|------|
| **A. Keep Soniox native translation, reinterpret force-chunk as watchdog only** | Minimal code. Honest model. No second truth source | Provider timing remains the constraint |
| **B. Add parallel chunked translator (DeepL/Google)** | Better forced timing control for long monologues | New code, split translation truth, mismatch risk, demo complexity |
| **C. Use Soniox-only short-term, design alternate path later if proven necessary** | Keeps demo surface area small | Does not solve provider limitation now |

### Recommendation

**Recommend A + C.**

Do not add a second translation path before the demo.

Treat this as an acknowledged provider constraint and manage it operationally:

- keep the watchdog
- measure semantic endpoint gaps
- trigger degradation when translation timing becomes unsafe

That is a better tradeoff than introducing a second translation control plane under deadline pressure.

---

## 2. No Graceful Degradation Ladder (Highest Priority — Reliability)

### Hole

This is the most important missing system behavior.

Right now the system has multiple recovery mechanisms, but no fully articulated fallback ladder for when translated cloned voice is no longer safe to present.

Without that, the product tends to keep trying to preserve the ideal experience even when:

- drift is growing
- TTS is failing
- translation is delayed
- queue pressure is increasing

That is the wrong failure philosophy for a demo system.

### Better Alternative

Implement a runtime degradation ladder with mechanical triggers.

Suggested ladder:

1. `Mode A`: translated cloned voice
2. `Mode B`: subtitles only
3. `Mode C`: original audio + subtitles
4. `Mode D`: live source stream + post-processed dubbed replay

Suggested triggers:

- drift exceeds threshold for sustained time
- repeated TTS timeouts
- repeated zero-byte TTS failures
- queue depth breach
- repeated long semantic endpoint gaps

Suggested recovery:

- degrade fast
- recover slowly
- avoid mode flapping

### Alternatives

| Option | Pros | Cons |
|--------|------|------|
| **A. Adaptive tier stepping** | Best reliability. Avoids catastrophic demo failure | Requires state handling and clear thresholds |
| **B. Per-language independent degradation** | Contains failures. One bad stream does not poison all | More state to monitor |
| **C. Current eviction-only behavior** | Simple | Silence gaps and visible instability |

### Recommendation

**This should be pre-demo work, not post-demo work.**

If only one architectural improvement gets made before the demo, it should be this one.

---

## 3. Prosody -> Emotion Pipeline Is Too Risky For Demo Use (High — Demo Risk, Low Effort)

### Hole

The current emotion pipeline:

- pitch autocorrelation
- energy RMS
- pause density
- 9 emotion classes
- voice-style mapping

is likely too fragile for the demo.

The failure mode is not subtle. If it guesses wrong, the cloned voice sounds inappropriate or uncanny. That is worse than sounding slightly flat.

This is especially risky across languages and speaking styles. A Korean host's delivery style does not cleanly map to a hand-built English-centric emotional taxonomy.

### Alternatives

| Option | Pros | Cons |
|--------|------|------|
| **A. Reduce to 3 states: neutral / high-energy / low-energy** | Safer. Language-agnostic enough. Small code change | Less expressive |
| **B. Disable adaptive emotion by default for demo** | Lowest risk. Most stable voice output | Less "wow" factor |
| **C. Host calibration phase** | Potentially more accurate | More code, more workflow complexity |

### Recommendation

**Recommend A or B before the demo.**

Do not spend time trying to perfect a 9-state classifier under current constraints.

---

## 4. Static Broadcast Delay Is Imperfect But Safer Than Adaptive Delay Right Now (Medium — Not Immediate)

### Hole

A fixed delay is always a compromise:

- too high for fast cases
- too low for slow cases

So yes, it is not globally optimal.

However, that does **not** mean adaptive delay is the right near-term fix.

### Alternatives

| Option | Pros | Cons |
|--------|------|------|
| **A. Conservative fixed delay per translated mode** | Stable. Predictable. Easy to reason about | Not optimal in best-case latency |
| **B. Per-mode fixed delay (normal vs backup TTS)** | More realistic than one-size-fits-all | Slightly more config complexity |
| **C. Fully adaptive delay** | Potential latency gains | More operational complexity, visible instability risk |

### Recommendation

**Recommend A or B before the demo.**

Adaptive delay is interesting later, but it adds behavioral complexity at exactly the layer where stability matters most.

---

## 5. TTS Caching Is Not A Core Design Hole Right Now (Low — Later Optimization)

### Hole

This is better framed as an optimization opportunity than a system-design hole.

Yes, live commerce contains repeated phrases. But in real delivery those repetitions often differ in:

- pacing
- surrounding context
- emphasis
- numbers
- urgency

A naive cache can easily return audio that sounds technically correct but contextually wrong or unnaturally repeated.

### Alternatives

| Option | Pros | Cons |
|--------|------|------|
| **A. No cache for live mode** | Safest | No latency savings |
| **B. Small exact-match cache for very stable phrases only** | Some savings with limited risk | Lower hit rate |
| **C. Phrase pre-warming for scripted content** | Strong demo value if script exists | Requires preparation workflow |

### Recommendation

Treat caching as a post-demo optimization.

If anything is done now, prefer controlled pre-warming of known scripted phrases over a broad live cache.

---

## 6. N+1 Connection Model Is Acceptable For Current Scope (Low — Future Scale Concern)

### Hole

The `N+1` model does scale linearly and can become a future operational issue at larger language counts.

That said, for the current target of 4+ languages, this is not the most important design problem.

The main issue is not raw connection count. It is making sure each language acts as an independent fault domain.

### Alternatives

| Option | Pros | Cons |
|--------|------|------|
| **A. Keep N+1, add explicit per-language state and degradation** | Minimal disruption. Better isolation | Still linear scaling |
| **B. 1 STT connection + N translation APIs** | Cleaner long-term decoupling | More code, more integration risk |
| **C. Vendor-specific multiplexing if available** | Lower connection count | Vendor-dependent and uncertain |

### Recommendation

Keep `N+1` for now, but make per-language failure isolation explicit.

For the demo, failure containment matters more than connection-count elegance.

---

## Revised Priority

| # | Hole | Risk | Effort | When |
|---|------|------|--------|------|
| 1 | No graceful degradation ladder | Reliability | Medium | Pre-demo |
| 2 | Translation accumulation during rapid speech | Stream quality | Medium | Pre-demo mitigation, not rewrite |
| 3 | Emotion/prosody overfitting | Voice quality | Low | Pre-demo |
| 4 | Static delay tuning | Latency + stability | Low/Medium | Pre-demo config only |
| 5 | TTS caching | Optimization | Low | Post-demo |
| 6 | N+1 scaling | Future scale | Low | Later |

## Final Recommendation

The original report is strongest when identifying:

- rapid-speech accumulation risk
- emotion misclassification risk
- the need for graceful degradation

It is weakest when recommending:

- a second translation path before the demo
- adaptive delay as an immediate improvement
- treating caching as a core architectural hole

The best pre-demo path is:

1. Keep Soniox native translation
2. Accept provider-owned segmentation as a hard constraint
3. Add explicit degradation behavior
4. Reduce adaptive voice variance
5. Use conservative translated-stream delay settings

## Bottom Line

The system does not need more algorithmic ambition before the demo.

It needs a clearer operational policy:

- when to trust the full pipeline
- when to degrade
- how to degrade safely
- how to avoid making a bad translated stream worse by trying too hard to preserve it
