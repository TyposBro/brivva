# Rust Stress Signals

Use this when inspecting `tmp/rust-stress-logs/**.log`.

## Good

- `speed=0.98x-1.02x` after warmup.
- `drop_frames=0` or rare.
- `buffered_chunks` stays near `0`.
- `tts_buffered_bytes` drains instead of growing forever.
- `tts_playback_speed` returns to `1.00` after catch-up.
- No `tts segment queue overflow` in normal healthy runs.
- YouTube shows healthy stream.

## Bad

- `speed <0.95x` for more than 30 seconds.
- `fifo_would_blocks` grows nonstop.
- `host_audio_stale_chunks_dropped`, `ready_host_bytes_dropped`, or
  `video_stale_chunks_dropped` grow during normal healthy runs.
- `tts_buffered_bytes` grows forever.
- `tts_playback_speed` stays above `1.00` for the whole run.
- `tts segment queue overflow` appears repeatedly.
- Repeated FFmpeg restarts.
- YouTube says not enough video or viewers buffer.

## Useful Greps

```bash
rg "test result|mp4 fanout smoke finished" tmp/rust-stress-logs/**/backlog_catchup_many_outputs.log
rg "tts segment queue overflow|tts_playback_speed|tts_catchup_active" tmp/rust-stress-logs/**/backlog_catchup_many_outputs.log
rg "drop_frames=|speed=|encode below realtime" tmp/rust-stress-logs/**/backlog_catchup_many_outputs.log
rg "video_stale_chunks_dropped|host_audio_stale_chunks_dropped|ready_host_bytes_dropped" tmp/rust-stress-logs/**/backlog_catchup_many_outputs.log
```
