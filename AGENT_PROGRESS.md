# AGENT_PROGRESS

## Current task
Implement GitHub issue #14: Use media timestamps for host audio buffering.

## Checklist
- [x] Read issue spec and current repo state.
- [x] Add timestamped host-audio clock mapper with reset/jump handling.
- [x] Add RTMP `push_host_audio_at` while preserving arrival-time fallback.
- [x] Wire timestamped `BTA2` frames to RTMP mapped capture time without affecting STT.
- [x] Add logs identifying audio clock source and resyncs.
- [x] Add tests for valid mapping, raw fallback, malformed frames, reset/jump, and jittered arrivals.
- [x] Run validation: `cargo test -p server-rs timestamped_audio --lib`, `cargo test -p server-rs ffmpeg --lib`, `cargo test -p server-rs session_ws --lib`, `cargo test -p server-rs`.
- [x] Commit stable changes.
- [x] Update GitHub issue/project status.

## Completed work
- Added `HostAudioClockMapper` for `BTA2` audio PTS → server `Instant` mapping.
- Added bounded resync handling for backward jumps, large media gaps, future mappings, and stale mappings.
- Added `RtmpManager::push_host_audio_at` and kept `push_host_audio` as arrival-time fallback.
- Wired timestamped audio frames to RTMP with mapped capture times while leaving STT PCM fanout unchanged.
- Added timeline-shadow payload fields and resync warning logs:
  - `audio_clock_source`
  - `audio_media_pts_us`
  - `audio_mapped_capture_age_ms`
  - `audio_clock_resync_count`
  - `audio_clock_resync_reason`

## Tests run
- `cargo test -p server-rs timestamped_audio --lib` ✅
- `cargo test -p server-rs ffmpeg --lib` ✅
- `cargo test -p server-rs session_ws --lib` ✅
- `cargo test -p server-rs` ✅

## Commits
- `fix(media): clock host audio by media pts`

## GitHub sync
- Issue #14 closed.
- Project #6 item for #14 moved to `Done`.

## Blockers
- None.

## Next action
Proceed to next ready project issue.
