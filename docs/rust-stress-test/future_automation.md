# Future Stress Automation

Useful smoke-test flags to add later:

- `MP4_FANOUT_SMOKE_AUDIO_DELAY_MS`
- `MP4_FANOUT_SMOKE_DROP_VIDEO_EVERY_N`
- `MP4_FANOUT_SMOKE_DROP_AUDIO_EVERY_N`
- `MP4_FANOUT_SMOKE_FAKE_BAD_RTMP=1`
- `MP4_FANOUT_SMOKE_TTS_DELAY_MS`
- `MP4_FANOUT_SMOKE_STT_DISABLE=1`

These avoid relying on OS-level `tc` or manual bad credentials for repeatable
chaos tests.
