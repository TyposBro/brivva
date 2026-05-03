# Rust Stress Commands

Use the ignored MP4 smoke test so the Rust server, FFmpeg, Soniox, ElevenLabs,
and RTMP publishers are exercised without frontend/Workers.

## Full Sequence

```bash
infisical run --env=dev --path=/ -- \
  ./scripts/run-rust-stress-tests.sh \
    --mp4 /home/typosbro/Desktop/text.mp4 \
    --4k-mp4 /home/typosbro/Desktop/4k-h264.mp4 \
    --source ko \
    --duration 60 \
    --long-duration 300
```

## Optional Inputs

```bash
infisical run --env=dev --path=/ -- \
  ./scripts/run-rust-stress-tests.sh \
    --mp4 /home/typosbro/Desktop/text.mp4 \
    --4k-mp4 /path/to/h264-4k.mp4 \
    --audio-mp4 /path/to/noisy-or-overlap.mp4 \
    --include-network \
    --iface <iface>
```

## Single Scenario

```bash
infisical run --env=dev --path=/ -- \
  ./scripts/run-rust-stress-tests.sh --only many_outputs_1080p30
```

The runner writes per-scenario logs under `tmp/rust-stress-logs/`.

Default translated-output media delay is `4000ms`. Override with
`RUST_STRESS_TRANSLATED_DELAY_MS=<ms>` when testing tighter or looser live
latency.

## Default Sequence

- `single_720p15`: weak camera profile, one output.
- `single_1080p30`: normal host profile, one output.
- `cpu_720p30`: CPU fallback ceiling, one output.
- `many_outputs_1080p30`: all available per-language YouTube outputs.
- `low_quality`: 720p15 input with 1080p30 caps, verifies no bad upscale/stutter.
- `backlog_catchup`: translated output with explicit 3s audio/video catch-up window.
- `backlog_catchup_many_outputs`: all available outputs with explicit 3s catch-up window.
- `single_4k30`: one 4K output, requires `--4k-mp4`.
- `many_outputs_4k30`: all available outputs at 4K30, requires `--4k-mp4`.
- `high_res_capped`: 4K input capped to 1080p30.
- `bad_destination`: one good output plus one dead RTMP destination.
- `tts_failure`: bad ElevenLabs key, original stream should continue.
- `stt_failure`: bad Soniox key, original stream should continue.
- `grip_smoke`: fresh Grip RTMP/RTMPS URL + key, verify Grip Studio receives
  video/audio.
- `long_run`: longer one-output run.
- `difficult_audio`: optional, requires `--audio-mp4`.
- `network`: optional, requires `--include-network --iface <iface>`.

## Manual Smoke Base

```bash
infisical run --env=dev --path=/ -- \
  env MP4_FANOUT_SMOKE_MP4=/home/typosbro/Desktop/text.mp4 \
      MP4_FANOUT_SMOKE_SOURCE_LANG=ko \
      MP4_FANOUT_SMOKE_DURATION=600 \
      MP4_FANOUT_SMOKE_ENCODER=nvenc \
      BRIVVA_VIDEO_MAX_WIDTH=1920 \
      BRIVVA_VIDEO_MAX_HEIGHT=1080 \
      BRIVVA_VIDEO_MAX_FPS=30 \
  cargo test -p server-rs --test mp4_fanout_smoke -- --ignored --nocapture
```

## Expected YouTube Secrets

```text
STREAM_KEY_YOUTUBE_PASS
STREAM_KEY_YOUTUBE_KO
STREAM_KEY_YOUTUBE_EN
STREAM_KEY_YOUTUBE_JA
STREAM_KEY_YOUTUBE_ZH
```

`PASS` is raw original. Source-language output, for example `KO` when
`MP4_FANOUT_SMOKE_SOURCE_LANG=ko`, is also original. Other languages use
translated TTS and burned subtitles.

The smoke feeder requires H.264 MP4 fixtures and copies H.264 into the Rust
server path. For AV1/HEVC/other source files, convert the fixture once before
running the smoke test; the live browser ingest path is H.264-only because RTMP
outputs require H.264.
