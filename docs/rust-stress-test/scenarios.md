# Rust Stress Scenarios

## CPU Fallback

Goal: know non-GPU behavior.

```bash
infisical run --env=dev --path=/ -- \
  env MP4_FANOUT_SMOKE_MP4=/home/typosbro/Desktop/text.mp4 \
      MP4_FANOUT_SMOKE_SOURCE_LANG=ko \
      MP4_FANOUT_SMOKE_DURATION=600 \
      MP4_FANOUT_SMOKE_ENCODER=x264 \
      BRIVVA_VIDEO_MAX_WIDTH=1920 \
      BRIVVA_VIDEO_MAX_HEIGHT=1080 \
      BRIVVA_VIDEO_MAX_FPS=30 \
  cargo test -p server-rs --test mp4_fanout_smoke -- --ignored --nocapture
```

Pass: 1080p30 stays near realtime. Fail means CPU fallback should use lower caps,
for example 720p30.

## Many Outputs

Goal: find practical encoder ceiling.

Set all five YouTube keys, then run the base command. This starts up to five
output groups: pass, source language, and translated languages.

Pass: all streams stay near realtime. Fail: reduce output resolution/FPS or move
to per-language GPU workers.

## High-Resolution Input

Goal: verify 4K/high-FPS sources downscale or preserve correctly.

Current status: not proven. Use this as a validation scenario, not evidence
that 4K is production-ready. The MP4 fixture must be H.264 because the real
browser ingest path is H.264-only for RTMP platform compatibility.

```bash
infisical run --env=dev --path=/ -- \
  env MP4_FANOUT_SMOKE_MP4=/path/to/h264-4k.mp4 \
      MP4_FANOUT_SMOKE_SOURCE_LANG=ko \
      MP4_FANOUT_SMOKE_DURATION=600 \
      MP4_FANOUT_SMOKE_ENCODER=nvenc \
      BRIVVA_VIDEO_MAX_WIDTH=3840 \
      BRIVVA_VIDEO_MAX_HEIGHT=2160 \
      BRIVVA_VIDEO_MAX_FPS=30 \
  cargo test -p server-rs --test mp4_fanout_smoke -- --ignored --nocapture
```

Then cap output to 1080p:

```bash
BRIVVA_VIDEO_MAX_WIDTH=1920 BRIVVA_VIDEO_MAX_HEIGHT=1080 BRIVVA_VIDEO_MAX_FPS=30
```

Pass: 4K source works when capped, and only uses 4K when GPU capacity is enough.

Fail/unknown:

- zero `chunks_written` means the fixture did not feed browser-like H.264 video
  into the Rust path;
- AV1/HEVC fixtures should be converted once to H.264 only to simulate browser
  ingest, not because production accepts arbitrary codecs;
- local 4K success still does not prove AWS 4K success.

Before claiming 4K support:

- run true 3840x2160 output profile;
- run 4K input capped to 1080p output;
- run all intended language/platform outputs;
- verify YouTube/Grip/TikTok Studio dashboards, not only Rust logs;
- repeat on AWS/Fargate exact launch instance and FFmpeg build.

## Low-Quality Host

Goal: simulate weak camera/laptop.

```bash
infisical run --env=dev --path=/ -- \
  env MP4_FANOUT_SMOKE_MP4=/home/typosbro/Desktop/text.mp4 \
      MP4_FANOUT_SMOKE_SOURCE_LANG=ko \
      MP4_FANOUT_SMOKE_DURATION=600 \
      MP4_FANOUT_SMOKE_ENCODER=nvenc \
      MP4_FANOUT_SMOKE_CAPTURE_WIDTH=1280 \
      MP4_FANOUT_SMOKE_CAPTURE_HEIGHT=720 \
      MP4_FANOUT_SMOKE_CAPTURE_FPS=15 \
      BRIVVA_VIDEO_MAX_WIDTH=1920 \
      BRIVVA_VIDEO_MAX_HEIGHT=1080 \
      BRIVVA_VIDEO_MAX_FPS=30 \
  cargo test -p server-rs --test mp4_fanout_smoke -- --ignored --nocapture
```

Pass: server does not upscale/stutter; output profile follows 720p15 tier.

## Bad Network

Goal: simulate RTMP backpressure and packet loss.

Find interface:

```bash
ip route get 8.8.8.8
```

Apply impairment:

```bash
sudo tc qdisc add dev <iface> root netem delay 150ms 50ms loss 1% rate 6mbit
```

Run base command. Remove impairment:

```bash
sudo tc qdisc del dev <iface> root
```

Pass: logs show degraded output clearly; process does not deadlock.

## Backlog Catch-Up

Goal: verify short FFmpeg/RTMP stalls preserve original audio, video, and TTS
within the live lag window before any forced drop. This scenario intentionally
selects a translated output, not `pass`, when a translated YouTube key exists.

```bash
infisical run --env=dev --path=/ -- \
  ./scripts/run-rust-stress-tests.sh --only backlog_catchup
```

Pass:

- `BRIVVA_VIDEO_MAX_LAG_MS=3000` and `BRIVVA_AUDIO_MAX_LAG_MS=3000` are active.
- Brief `fifo_would_blocks` does not consume TTS; `tts_buffered_bytes` later drains.
- `requeued_host_bytes` appears only during FIFO backpressure.
- `host_audio_stale_chunks_dropped`, `ready_host_bytes_dropped`, and
  `video_stale_chunks_dropped` stay `0` unless the stream exceeds the lag window.

## Backlog Catch-Up With All Outputs

Goal: verify the same 3s backlog policy under real fanout load.

```bash
infisical run --env=dev --path=/ -- \
  ./scripts/run-rust-stress-tests.sh --only backlog_catchup_many_outputs
```

Pass:

- All available outputs start.
- Translated outputs show TTS activity and eventually drain `tts_buffered_bytes`.
- `tts_playback_speed` may rise above `1.00` during backlog, then returns to
  `1.00`.
- No repeated `tts segment queue overflow` during healthy 1080p30 fanout.
- Per-output `video_stale_chunks_dropped`, `host_audio_stale_chunks_dropped`,
  and `ready_host_bytes_dropped` stay `0` unless total load exceeds the lag window.
- If a drop happens, logs identify which output/lang dropped.

## One Bad Destination

Goal: one failed platform must not kill others.

```bash
MP4_FANOUT_SMOKE_RTMP_URLS=rtmp://127.0.0.1:1/live/bad
```

Run base command with normal YouTube keys too.

Pass: bad destination logs failure; valid YouTube outputs remain live.

## Mobile-First RTMP Output

Default RTMP output profile for YouTube, Grip, TikTok, and generic RTMP:

- Codec: H.264.
- Canvas: exact `720x1280` portrait. Landscape source is fit inside this
  portrait canvas with padding.
- Keyframe interval: 1 second.
- Video bitrate: `2500k`, maxrate `2800k`; with AAC audio this stays below
  Grip's "less than 3 Mbps" requirement.

Reason: Brivva's launch use case is live commerce, and most viewers are on
phones. YouTube can accept both portrait and landscape, so portrait is the safer
default across platforms.

Override only for explicit desktop/4K experiments:

```bash
BRIVVA_RTMP_OUTPUT_LAYOUT=source
```

## Grip Smoke

Goal: prove Grip RTMP/RTMPS publish works with a fresh one-shot Grip stream key.

Required secrets/env:

- `STREAM_URL_GRIP`: Grip server URL from PC 송출.
- `STREAM_KEY_GRIP`: fresh Grip stream key for this broadcast.
- Optional `MP4_FANOUT_SMOKE_GRIP_LANG`: output language; defaults to source
  language.

Run through the stress wrapper:

```bash
infisical run --env=dev --path=/ -- \
  ./scripts/run-rust-stress-tests.sh --only grip_smoke
```

Or run only the Rust smoke test:

```bash
infisical run --env=dev --path=/ -- \
  env MP4_FANOUT_SMOKE_MP4=/home/typosbro/Desktop/text.mp4 \
      MP4_FANOUT_SMOKE_OUTPUTS=grip \
      MP4_FANOUT_SMOKE_DURATION=180 \
      MP4_FANOUT_SMOKE_ENCODER=nvenc \
      BRIVVA_VIDEO_MAX_WIDTH=1920 \
      BRIVVA_VIDEO_MAX_HEIGHT=1080 \
      BRIVVA_VIDEO_MAX_FPS=30 \
  cargo test -p server-rs --test mp4_fanout_smoke -- --ignored --nocapture
```

Pass:

- Rust logs show Grip output started and `chunks_written` increases.
- `ffmpeg spawn` for `destination_platform=grip` shows `max_width=720`,
  `max_height=1280`, `bitrate_kbps=2500`, and
  `keyframe_interval_frames=30`.
- FFmpeg stderr stays near realtime.
- Grip Studio monitor shows video and audio.
- No `ffmpeg rtmp process crashed`.

Important: Grip stream keys are one-shot. Do not reuse an old key for this
test.

Important: `backlog_catchup_many_outputs` is YouTube-only by design. It does
not prove Grip, even if it passes. Use `--only grip_smoke` when watching Grip
Studio.

Default: if Grip creds exist, single-output stress scenarios use Grip first.
This matches the phone-first production audience. Many-output scenarios still
avoid Grip by default because Grip stream keys are one-shot.

## TTS Failure

Goal: translated audio failure must not kill original stream.

Run with wrong ElevenLabs key:

```bash
ELEVENLABS_API_KEY=bad
```

Pass: source/pass streams continue. Translated streams show subtitle/TTS failure
logs, but video does not stall.

## STT Failure

Goal: STT failure must not kill original stream.

Run with wrong Soniox key:

```bash
SONIOX_API_KEY=bad
```

Pass: source/pass streams continue. Target translation streams degrade clearly.

## Long Run

Goal: catch memory growth, queue drift, slow leaks.

```bash
MP4_FANOUT_SMOKE_DURATION=3600
```

Pass: memory stable; `tts_buffered_bytes`, `fifo_would_blocks`, and restarts do
not grow without bound.

## Difficult Audio

Goal: test translation quality and queue behavior, not encoder.

Try MP4 files with:

- silence
- background music
- overlapping speakers
- noisy mic
- very fast speech
- code-switching Korean/English

Pass: original stream remains stable; STT/TTS errors are visible and bounded.
