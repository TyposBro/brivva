# Updated System Design Report

## Executive Summary

The current architecture is directionally correct for the April 16, 2026 demo, but it has one important structural hole:

- Soniox owns real translation boundaries.
- The local pipeline still behaves as if it can meaningfully control them.

This is the core contradiction in the current system design.

The right response is **not** to build a custom segmentation layer. That would add code, complexity, and risk under a tight deadline. The better path is to accept provider-owned segmentation as a hard constraint and redesign the system's fallback behavior around that fact.

The most important missing capability is not smarter chunking. It is an explicit, automatic **degradation policy** for when translation timing, TTS, or sync fall outside the safe operating envelope.

## Current Design Assessment

The current pipeline is:

```text
Host Audio -> N+1 Soniox v4 WebSocket connections
           -> Semantic endpointing + native translation
           -> 4-second force-chunk watchdog
           -> Prosody analysis -> emotion classification -> voice style mapping
           -> ElevenLabs TTS (WebSocket + REST fallback) + DashScope backup
           -> StreamingPcm -> FFmpeg audio drain -> RTMP push

Host Video -> MediaRecorder -> delayed buffer -> FFmpeg -> RTMP push
```

This design has real strengths:

- It keeps the pipeline relatively simple.
- It avoids rebuilding translation infrastructure that the provider already offers.
- It preserves the most valuable local engineering: A/V sync, RTMP output, buffering, and crash recovery.
- It fits the project principle that unnecessary code is a liability.

Those are good decisions.

## Main Hole In The Current Design

The system currently uses several local controls to manage timing:

- 4-second force-chunk threshold
- stale `play_at` capping
- queue depth limits
- staleness eviction
- speech-duration padding

These controls help, but they do not solve the underlying issue:

Soniox can internally accumulate translation for long continuous speech and emit it only when its semantic endpointing decides to do so. Local force-chunking does not change that internal provider behavior.

As a result:

- local timers can detect delay, but not prevent it
- local scheduling can smooth output timing, but not restore semantic alignment
- duration sync can improve while meaning-level sync still drifts

This means the architecture is currently stronger at **damage control** than at **timing control**.

That is acceptable if the system explicitly acknowledges it and degrades safely. It is risky if the system keeps trying to preserve full live translated cloned voice under conditions where the provider timing is already outside the safe envelope.

## Why This Matters For The Demo

The demo priorities are already clear:

1. Voice cloning quality
2. Reliability
3. A/V sync
4. Latency

That ordering matters.

A stream that is slightly slower but stable is acceptable. A stream with robotic voice, broken timing, or visible sync drift is not. The customer already has a human alternative. The product only wins if it is operationally trustworthy.

Because of that, the system should optimize for:

- predictable behavior under stress
- visible graceful degradation
- avoiding obviously bad translated voice output

It should not optimize for preserving the "ideal" pipeline at all costs.

## Recommended Changes

### 1. Treat Soniox Semantic Endpointing As Authoritative

Do not treat the 4-second force-chunk path as real segmentation. Treat it as:

- a watchdog
- a telemetry signal
- a trigger for downgrade decisions

This is the cleanest interpretation of current reality.

Pros:

- Minimal new code
- More honest system model
- Fewer false assumptions in scheduling logic

Cons:

- Latency variance remains provider-limited
- Long monologues are still a weak case

### 2. Add A Hard Degradation Ladder

This is the single most important architectural addition.

Recommended runtime ladder:

1. `Mode A`: translated cloned voice
2. `Mode B`: subtitles only
3. `Mode C`: original audio + subtitles
4. `Mode D`: live source stream + post-processed dubbed replay

This should happen automatically, not by manual operator judgment.

Recommended downgrade triggers:

- drift exceeds threshold for sustained duration
- repeated TTS timeout
- repeated zero-byte or unusable TTS response
- queue depth breach
- repeated long semantic endpoint gaps

Recommended recovery behavior:

- only promote back upward after sustained healthy operation
- avoid rapid oscillation between modes
- make mode changes visible in telemetry and UI

Pros:

- Biggest reliability gain for the least new code
- Prevents the worst demo failure modes
- Aligns the system with business priorities

Cons:

- Some sessions visibly degrade
- Product story becomes less magical but more honest

### 3. Increase Default Delay For Translated Streams

The current default delay is 3000ms. That may be too aggressive for the translated path if the real priority is reliability and sync.

Recommendation:

- keep source-language passthrough fast
- allow translated streams to use a more conservative default delay
- use even more headroom when backup TTS is active

Pros:

- Immediate stability improvement
- More room for provider variability
- Lower sync pressure on the TTS side

Cons:

- Higher visible latency
- Less impressive headline number

### 4. Make Per-Language Failure Isolation Explicit

`N+1` connections are acceptable if each target language is treated as its own fault domain.

Each language stream should have an explicit state model for:

- health
- drift
- queue pressure
- last successful translation
- last successful TTS
- current output mode

This does not require a radical rewrite. It requires making the existing operational model explicit.

Pros:

- Better fault containment
- Easier diagnosis during live sessions
- Safer multi-language demos

Cons:

- More orchestration logic
- More metrics and UI state

### 5. Reduce Adaptive Voice Styling For The Demo

Prosody-to-emotion-to-style mapping is interesting, but it adds variability to the output customers will judge most harshly.

Recommendation:

- preserve voice identity
- reduce aggressive style modulation
- only enable expressive adjustments when confidence is high

Pros:

- More stable perceived quality
- Lower uncanny-risk
- Less chance of overreactive voice behavior

Cons:

- Less expressive speech
- Slightly less impressive on ideal inputs

## Alternatives Considered

### Alternative 1: Controlled Segmentation Architecture

Description:

- use Soniox mainly for source transcript/interims
- segment locally
- translate and synthesize explicit units under local ownership

Pros:

- better timing control
- stronger provider swap story
- cleaner scheduling model

Cons:

- more code
- more moving parts
- higher implementation risk before demo

Decision:

Rejected for now. It violates the current engineering constraint that unnecessary code should not be added unless it changes outcomes enough to justify the complexity.

### Alternative 2: Keep Current Architecture But Reinterpret Local Timers

Description:

- keep Soniox native translation
- keep local force-chunk logic
- stop treating local timers as true segmentation
- use them only for watchdog and downgrade logic

Pros:

- minimal code
- realistic
- strong near-term path

Cons:

- provider timing still dominates
- does not improve best-case latency

Decision:

Recommended.

### Alternative 3: Preserve Full Translated Voice At All Costs

Description:

- continue trying to keep live cloned translated voice active even when drift and provider delay are visibly unsafe

Pros:

- maximum product ambition

Cons:

- highest demo risk
- worst user-visible failures
- inconsistent with business priorities

Decision:

Not recommended.

## Suggested Operational Policy

The system should explicitly distinguish between:

- **ideal mode**
- **safe degraded mode**
- **recovery mode**

Suggested policy:

- If translated audio is healthy, stay in translated cloned voice mode.
- If timing becomes unsafe, degrade quickly.
- If recovery is stable, promote slowly.
- Never allow repeated flapping between modes.

This makes the product operationally trustworthy even when provider behavior is not fully controllable.

## Demo-Specific Go/No-Go Criteria

Before the April 16, 2026 demo, the system should meet these gates:

### Go Criteria

- 30-minute endurance run without crash
- no visible sustained A/V sync failure
- no repeated TTS failure loops
- no queue runaway in any target language
- degraded mode triggers correctly when limits are breached
- operator can explain the fallback behavior clearly

### No-Go Criteria

- translated voice continues after drift is obviously unsafe
- the system piles up multiple stale utterances
- one unhealthy language can destabilize the whole session
- backup TTS path causes persistent sync loss
- the product hides failure instead of degrading explicitly

## Final Recommendation

Do not build a custom segmentation architecture right now.

The better design is:

- keep Soniox native translation
- accept provider-owned segmentation as a hard constraint
- reinterpret force-chunk as watchdog logic, not real chunk control
- add an automatic degradation ladder
- increase delay headroom for translated paths
- make per-language failure state explicit
- reduce adaptive styling variance for the demo

## Bottom Line

The biggest hole in the current system is not missing algorithmic sophistication.

It is the absence of a clear policy for when the system should stop trying to preserve the ideal live translated cloned-voice experience and fall back to a safer one.

Given the deadline and the product priorities, the best alternative is not a smarter pipeline. It is a more honest one.
