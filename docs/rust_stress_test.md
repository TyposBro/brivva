# Rust Server Stress Test

Purpose: prove `server-rs` live media path works outside ideal local conditions.
Use the ignored MP4 smoke test so the Rust server, FFmpeg, Soniox, ElevenLabs,
and RTMP publishers are exercised without frontend/Workers.

## Base Command

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

Expected YouTube secrets:

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

## Signals

Good:

- `speed=0.98x-1.02x` after warmup.
- `drop_frames=0` or rare.
- `buffered_chunks` stays near `0`.
- `tts_buffered_bytes` drains instead of growing forever.
- YouTube shows healthy stream.

Bad:

- `speed <0.95x` for more than 30 seconds.
- `fifo_would_blocks` grows nonstop.
- `tts_buffered_bytes` grows forever.
- Repeated FFmpeg restarts.
- YouTube says not enough video or viewers buffer.

## Scenarios

### 1. CPU Fallback

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

### 2. Many Outputs

Goal: find practical encoder ceiling.

Set all five YouTube keys, then run the base command. This starts up to five
output groups: pass, source language, and translated languages.

Pass: all streams stay near realtime. Fail: reduce output resolution/FPS or move
to per-language GPU workers.

### 3. High-Resolution Input

Goal: verify 4K/high-FPS sources downscale or preserve correctly.

```bash
infisical run --env=dev --path=/ -- \
  env MP4_FANOUT_SMOKE_MP4=/path/to/4k.mp4 \
      MP4_FANOUT_SMOKE_SOURCE_LANG=ko \
      MP4_FANOUT_SMOKE_DURATION=600 \
      MP4_FANOUT_SMOKE_ENCODER=nvenc \
      BRIVVA_VIDEO_MAX_WIDTH=3840 \
      BRIVVA_VIDEO_MAX_HEIGHT=2160 \
      BRIVVA_VIDEO_MAX_FPS=60 \
  cargo test -p server-rs --test mp4_fanout_smoke -- --ignored --nocapture
```

Then cap output to 1080p:

```bash
BRIVVA_VIDEO_MAX_WIDTH=1920 BRIVVA_VIDEO_MAX_HEIGHT=1080 BRIVVA_VIDEO_MAX_FPS=30
```

Pass: 4K source works when capped, and only uses 4K when GPU capacity is enough.

### 4. Low-Quality Host

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

### 5. Bad Network

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

### 6. One Bad Destination

Goal: one failed platform must not kill others.

```bash
MP4_FANOUT_SMOKE_RTMP_URLS=rtmp://127.0.0.1:1/live/bad
```

Run base command with normal YouTube keys too.

Pass: bad destination logs failure; valid YouTube outputs remain live.

### 7. TTS Failure

Goal: translated audio failure must not kill original stream.

Run with wrong ElevenLabs key:

```bash
ELEVENLABS_API_KEY=bad
```

Pass: source/pass streams continue. Translated streams show subtitle/TTS failure
logs, but video does not stall.

### 8. STT Failure

Goal: STT failure must not kill original stream.

Run with wrong Soniox key:

```bash
SONIOX_API_KEY=bad
```

Pass: source/pass streams continue. Target translation streams degrade clearly.

### 9. Long Run

Goal: catch memory growth, queue drift, slow leaks.

```bash
MP4_FANOUT_SMOKE_DURATION=3600
```

Pass: memory stable; `tts_buffered_bytes`, `fifo_would_blocks`, and restarts do
not grow without bound.

### 10. Difficult Audio

Goal: test translation quality and queue behavior, not encoder.

Try MP4 files with:

- silence
- background music
- overlapping speakers
- noisy mic
- very fast speech
- code-switching Korean/English

Pass: original stream remains stable; STT/TTS errors are visible and bounded.

## Next Automation

Useful smoke-test flags to add later:

- `MP4_FANOUT_SMOKE_AUDIO_DELAY_MS`
- `MP4_FANOUT_SMOKE_DROP_VIDEO_EVERY_N`
- `MP4_FANOUT_SMOKE_DROP_AUDIO_EVERY_N`
- `MP4_FANOUT_SMOKE_FAKE_BAD_RTMP=1`
- `MP4_FANOUT_SMOKE_TTS_DELAY_MS`
- `MP4_FANOUT_SMOKE_STT_DISABLE=1`

These avoid relying on OS-level `tc` or manual bad credentials for repeatable
chaos tests.
