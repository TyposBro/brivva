# Plan: Source Speech Timing vs Available Live Window

Goal: use silence after fast host utterances without lying about source speech duration.

Current RS-023 patch fixed the obvious bug: TTS no longer estimates host source duration from translated text length. It now carries source timing from Soniox timestamps when available.

This next plan is **not** a quick accumulator hack. It introduces a clean distinction:

```rust
pub source_speech_duration_ms: u64,
pub available_window_ms: Option<u64>,
pub timing_method: SourceTimingMethod,
```

TTS policy can use `available_window_ms` for concision decisions, while logs/diagnostics still use `source_speech_duration_ms` for true expansion ratio.

## Problem This Solves

Example:

```text
Host says quickly: "Hi, how are you doing?"
Actual spoken duration: 200ms
Then host is silent: 2s
```

If TTS budget only uses spoken duration, JA/ZH/KO translation gets forced into ~500ms after clamp. That causes unnecessary concision/speedup and worse quality.

But translated audio can safely use some of the silence before the next host utterance.

Correct model:

```text
source_speech_duration_ms = current_source_end_ms - current_source_start_ms
available_window_ms = next_source_start_ms - current_source_start_ms
```

Use:

- `source_speech_duration_ms` for actual expansion diagnostics;
- `available_window_ms` for pre-synthesis TTS budget/concision, when known.

## Non-Negotiable Design Rule

Do **not** pretend:

```text
source duration = next utterance start - current utterance start
```

That would corrupt metrics.

Instead, keep two separate fields:

```rust
source_speech_duration_ms // truth: how long host actually spoke
available_window_ms       // budget: how much live time TTS can occupy
```

## Proposed Domain Types

File: `server-rs/src/features/broadcast/domain/mod.rs`

Current:

```rust
pub struct SourceUtteranceTiming {
    pub duration_ms: u64,
    pub method: SourceTimingMethod,
}
```

Replace with:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceTimingMethod {
    SonioxTokenTimestamps,
    ResponseWallClock,
    TextEstimateFallback,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AvailableWindowMethod {
    NextUtteranceStart,
    HoldTimeoutFallback,
    SameAsSpeech,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceUtteranceTiming {
    pub source_speech_duration_ms: u64,
    pub available_window_ms: Option<u64>,
    pub timing_method: SourceTimingMethod,
    pub available_window_method: AvailableWindowMethod,
}
```

Helper:

```rust
impl SourceUtteranceTiming {
    pub fn tts_budget_ms(self) -> u64 {
        self.available_window_ms
            .unwrap_or(self.source_speech_duration_ms)
            .clamp(500, MAX_TTS_BUDGET_MS)
    }
}
```

Initial cap:

```rust
const MAX_TTS_BUDGET_MS: u64 = 3_000;
```

Rationale: use silence, but do not let 10s silence make translation bloated.

## Architecture: Hold Buffer / Scheduler

Current flow:

```text
Soniox flushes utterance N
-> emit Translation
-> dispatch TTS immediately
```

New flow:

```text
Soniox flushes utterance N
-> emit Translation immediately
-> hold TTS dispatch briefly
-> if utterance N+1 starts soon, compute N.available_window_ms
-> dispatch N TTS with better budget
-> if no N+1 arrives before timeout, dispatch N with fallback budget
```

Only TTS dispatch is delayed. UI translation/subtitle should stay immediate.

## Why Scheduler Instead Of Accumulator Hack

Accumulator knows current utterance timing only. It does not know the next utterance start yet.

If we mutate current duration later inside accumulator, bugs appear:

- duplicate dispatch;
- final utterance never dispatches;
- long silence creates huge budget;
- per-language streams diverge silently;
- logs confuse speech duration and usable budget.

Scheduler/hold-buffer makes state explicit:

```text
pending utterance waiting for either:
1. next utterance start, or
2. timeout
```

## Actionable Implementation Steps

### Step 1 — Refactor `SourceUtteranceTiming` fields

File: `server-rs/src/features/broadcast/domain/mod.rs`

Change from:

```rust
pub duration_ms: u64,
pub method: SourceTimingMethod,
```

to:

```rust
pub source_speech_duration_ms: u64,
pub available_window_ms: Option<u64>,
pub timing_method: SourceTimingMethod,
pub available_window_method: AvailableWindowMethod,
```

Update all callsites/tests.

For first compile-safe migration:

```rust
available_window_ms: None,
available_window_method: AvailableWindowMethod::Unavailable,
```

### Step 2 — Update TTS policy to use budget helper

File: `server-rs/src/features/broadcast/data/pipeline/tts.rs`

Current logic uses:

```rust
let estimated_source_duration_ms = source_timing.duration_ms;
```

Change to:

```rust
let source_speech_duration_ms = source_timing.source_speech_duration_ms;
let tts_budget_ms = source_timing.tts_budget_ms();
```

Use `tts_budget_ms` for:

- `predict_tts_policy()` denominator;
- `live_commerce_text_for_policy_with_budget()`;
- pre-synthesis concision decisions.

Use `source_speech_duration_ms` for:

- actual expansion logs;
- diagnostics.

Log both:

```rust
source_speech_duration_ms,
available_window_ms = ?source_timing.available_window_ms,
tts_budget_ms,
timing_method = ?source_timing.timing_method,
available_window_method = ?source_timing.available_window_method,
```

### Step 3 — Preserve source start/end in STT accumulator

File: `server-rs/src/features/broadcast/data/pipeline/stt_response.rs`

Current accumulator computes duration from min start/max end. Keep raw boundaries too:

```rust
source_start_ms: Option<u64>,
source_end_ms: Option<u64>,
```

Create completed utterance object:

```rust
struct CompletedTranslationUtterance {
    committed: String,
    utterance_id: u64,
    target_lang: Lang,
    source_start_ms: Option<u64>,
    source_end_ms: Option<u64>,
    fallback_speech_duration_ms: u64,
    timing_method: SourceTimingMethod,
    selected_voice_id: Option<String>,
    selected_voice_enrollment_lang: Option<Lang>,
    voice_preset: VoicePreset,
}
```

Speech duration builder:

```rust
source_speech_duration_ms = match (source_start_ms, source_end_ms) {
    (Some(start), Some(end)) => end.saturating_sub(start).clamp(500, 15_000),
    _ => fallback_wall_clock_or_text_estimate,
}
```

### Step 4 — Split translation emit from TTS dispatch

Currently `emit_translation()` both:

- sends frontend translation/subtitle;
- dispatches TTS.

Split into:

```rust
emit_translation_to_frontend(...)
schedule_translation_tts(...)
```

Frontend/subtitle stays immediate.

TTS goes through scheduler.

### Step 5 — Add pending TTS hold buffer

Inside each translation response processor, keep:

```rust
let pending_tts: Arc<Mutex<Option<CompletedTranslationUtterance>>> = ...;
```

When utterance N completes:

1. If pending N-1 exists and N has `source_start_ms`, dispatch N-1 with:

```rust
available_window_ms = N.source_start_ms - N_minus_1.source_start_ms
available_window_method = AvailableWindowMethod::NextUtteranceStart
```

2. Store N as pending.
3. Arm timeout for N.

### Step 6 — Add hold timeout

Constant:

```rust
const TTS_LOOKAHEAD_HOLD_TIMEOUT_MS: u64 = 500;
```

If no next utterance arrives within 500ms, dispatch pending N with:

```rust
available_window_ms = None
available_window_method = AvailableWindowMethod::HoldTimeoutFallback
```

TTS budget helper then falls back to `source_speech_duration_ms`, maybe with a quality floor:

```rust
source_speech_duration_ms.max(1_000).min(3_000)
```

Need decision:

- strict fallback: use speech only;
- quality fallback: allow minimum 1s budget after timeout.

Recommended first version: quality fallback, because timeout itself means host silence existed for 500ms.

### Step 7 — Prevent duplicate dispatch

Timeout and next-utterance path can race.

Required implementation pattern:

```rust
let pending = pending_tts.lock().await.take();
if let Some(pending) = pending {
    dispatch(pending);
}
```

Only `take()` owner dispatches.

If timeout already dispatched, next utterance sees `None` and does not dispatch old utterance again.

### Step 8 — Env flag rollout

Do not make scheduler default immediately.

Add:

```bash
BRIVVA_TTS_LOOKAHEAD_BUDGET=1
```

When off:

- immediate dispatch;
- `available_window_ms = None`;
- behavior equivalent to current RS-023 patch, except new explicit fields/logs.

When on:

- hold buffer active;
- next-start budget active.

### Step 9 — Tests

Add tests for these exact cases.

#### Domain/TTS tests

1. `tts_budget_ms()` uses `available_window_ms` when present.
2. `tts_budget_ms()` falls back to `source_speech_duration_ms` when absent.
3. huge available window is capped at `MAX_TTS_BUDGET_MS`.
4. TTS policy is less aggressive with:

```rust
source_speech_duration_ms = 500
available_window_ms = Some(2_000)
```

than with only 500ms speech.

#### STT scheduler tests

5. Fast utterance followed by silence/next utterance:
   - N speech = 500ms clamp;
   - N+1 starts 2,000ms after N start;
   - N dispatched with `available_window_ms=Some(2_000)`.

6. Huge silence:
   - N+1 starts 10,000ms later;
   - budget helper caps to 3,000ms.

7. No next utterance:
   - timeout dispatches N.

8. No duplicate dispatch:
   - timeout dispatches N;
   - later N+1 arrives;
   - N was sent exactly once.

9. Missing Soniox timestamps:
   - scheduler falls back cleanly;
   - logs/timing method show fallback.

10. Frontend translation is immediate:
   - `ServerMsg::Translation` sent before TTS hold timeout.

### Step 10 — Validation

Run:

```bash
cargo test -p server-rs stt_response
cargo test -p server-rs tts
cargo test -p server-rs ffmpeg
cargo check -p server-rs
git diff --check
```

Then live/stress with flag on:

```bash
BRIVVA_TTS_LOOKAHEAD_BUDGET=1
```

Inspect logs for:

```text
source_speech_duration_ms=500
available_window_ms=Some(2000)
tts_budget_ms=2000
available_window_method=NextUtteranceStart
```

## Risk Assessment

Feasible, but more dangerous than RS-023 because it introduces delayed dispatch and race potential.

Primary risk areas:

- duplicate TTS dispatch;
- final utterance stuck pending;
- latency increase;
- per-language divergence;
- scheduler state surviving session teardown;
- timeout too short/long.

Mitigation:

- env flag;
- `take()`-based single-dispatch ownership;
- keep frontend translation immediate;
- cap available window;
- tests for timeout and duplicate dispatch.

## Recommended Patch Order

### Patch A — Safe field split

- introduce `source_speech_duration_ms` and `available_window_ms` fields;
- TTS uses helper budget;
- no scheduler yet;
- behavior equivalent to current.

### Patch B — Scheduler behind env flag

- hold buffer;
- next-start budget;
- timeout;
- no duplicate dispatch tests.

### Patch C — Live validation and default-on decision

- run stress/live logs;
- listen to JA/ZH/KO quality;
- if good, make default.

## Go / No-Go

Go only if scheduler can be implemented with small, testable local state.

No-go if it requires broad STT pipeline architecture changes. In that case keep RS-023 patch, collect live timing logs, then revisit with a dedicated scheduler refactor.
