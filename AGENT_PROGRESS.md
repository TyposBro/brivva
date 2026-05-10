# AGENT_PROGRESS

## Current task
Implement GitHub issue #15: Add server-side audio limiter and ducking.

## Checklist
- [x] Read issue spec and current repo state.
- [x] Add limiter/mix stats primitives in `mixer.rs`.
- [x] Add configurable limiter default-on and ducking default-off controls.
- [x] Wire limiter after host/TTS mix before FFmpeg FIFO write.
- [x] Apply optional ducking only on translated TTS-active ticks.
- [x] Add periodic stream-level clipping/limiter/ducking logs.
- [x] Add unit tests for limiter, clipping stats, ducking, odd-byte/silence, source/pass-through natural path.
- [x] Run validation: `cargo test -p server-rs mixer --lib`, `cargo test -p server-rs ffmpeg --lib`, `cargo test -p server-rs`.
- [x] Commit and push.
- [x] Close issue #15 and move project item to Done.

## Completed work
- Added mixer primitives:
  - `mix_pcm_s16le_with_stats`
  - `analyze_pcm_s16le`
  - `count_clipped_samples`
  - `limit_pcm_s16le`
  - `duck_gain`
- Added default-on limiter with rollback env `BRIVVA_AUDIO_LIMITER_ENABLED=0`.
- Added opt-in ducking envs `BRIVVA_AUDIO_DUCKING_ENABLED=1` and `BRIVVA_AUDIO_DUCKING_DB`.
- Wired limiter after source/translated tick construction and before FIFO write.
- Wired ducking only for translated streams when non-zero TTS PCM is active; source streams skip ducking and TTS consumption.
- Added per-second audio quality logs/tracing fields:
  - `audio_peak_before_limiter`
  - `audio_peak_after_limiter`
  - `audio_limited_samples`
  - `audio_clipped_samples_pre_limiter`
  - `audio_ducked_ticks`
  - `audio_ducking_db`
  - `lang`, `destination_platform`, `stream_id`, `passthrough`, `is_source`

## Tests run
- `cargo test -p server-rs mixer --lib` ✅
- `cargo test -p server-rs ffmpeg --lib` ✅
- `cargo test -p server-rs` ✅

## Commits
- `7d6dbd1 fix(audio): limit and duck mixed output`

## GitHub sync
- Issue #15 closed.
- Project #6 item for #15 moved to `Done`.

## Blockers
- None.

## Next action
Proceed to next ready project issue.
