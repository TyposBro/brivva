# Future Stress Automation

Implemented smoke-test chaos flags:

- `MP4_FANOUT_SMOKE_AUDIO_DELAY_MS`
- `MP4_FANOUT_SMOKE_DROP_VIDEO_EVERY_N`
- `MP4_FANOUT_SMOKE_DROP_AUDIO_EVERY_N`
- `MP4_FANOUT_SMOKE_FAKE_BAD_RTMP=1`
- `MP4_FANOUT_SMOKE_TTS_DELAY_MS`
- `MP4_FANOUT_SMOKE_STT_DISABLE=1`

These avoid relying on OS-level `tc` or manual bad credentials for repeatable
chaos tests.

Still useful later:

- JSON `summary.json` per stress run.
- Local RTMP sink process for deterministic one-bad-destination validation.
- CI wrapper that runs short chaos scenarios against local-only destinations.
