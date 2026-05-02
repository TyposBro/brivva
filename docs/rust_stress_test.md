# Rust Server Stress Test

Purpose: prove `server-rs` live media path works outside ideal local conditions.
Use the ignored MP4 smoke test so the Rust server, FFmpeg, Soniox, ElevenLabs,
and RTMP publishers are exercised without frontend/Workers.

## Base Command

Run the full automated sequence:

```bash
infisical run --env=dev --path=/ -- \
  ./scripts/run-rust-stress-tests.sh \
    --mp4 /home/typosbro/Desktop/text.mp4 \
    --4k-mp4 /home/typosbro/Desktop/4k-h264.mp4 \
    --source ko \
    --duration 60 \
    --long-duration 300
```

Optional:

```bash
infisical run --env=dev --path=/ -- \
  ./scripts/run-rust-stress-tests.sh \
    --mp4 /home/typosbro/Desktop/text.mp4 \
    --4k-mp4 /path/to/h264-4k.mp4 \
    --audio-mp4 /path/to/noisy-or-overlap.mp4 \
    --include-network \
    --iface <iface>
```

Run one scenario:

```bash
infisical run --env=dev --path=/ -- \
  ./scripts/run-rust-stress-tests.sh --only many_outputs_1080p30
```

The runner writes per-scenario logs under `tmp/rust-stress-logs/`.

Default full sequence:

- `single_720p15`: weak camera profile, one output.
- `single_1080p30`: normal host profile, one output.
- `cpu_720p30`: CPU fallback ceiling, one output.
- `many_outputs_1080p30`: all available per-language YouTube outputs.
- `low_quality`: 720p15 input with 1080p30 caps, verifies no bad upscale/stutter.
- `backlog_catchup`: translated output with explicit 3s audio/video catch-up window; validates host audio, video, and TTS backlog policy logs.
- `backlog_catchup_many_outputs`: all available outputs with explicit 3s audio/video catch-up window.
- `single_4k30`: one 4K output, requires `--4k-mp4`.
- `many_outputs_4k30`: all available outputs at 4K30, requires `--4k-mp4`.
- `high_res_capped`: 4K input capped to 1080p30.
- `bad_destination`: one good output plus one dead RTMP destination.
- `tts_failure`: bad ElevenLabs key, original stream should continue.
- `stt_failure`: bad Soniox key, original stream should continue.
- `long_run`: longer one-output run.
- `difficult_audio`: optional, requires `--audio-mp4`.
- `network`: optional, requires `--include-network --iface <iface>`.

Manual single-scenario base command:

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

The smoke feeder requires H.264 MP4 fixtures and copies H.264 into the Rust
server path. For AV1/HEVC/other source files, convert the fixture once before
running the smoke test; the live browser ingest path is H.264-only because RTMP
outputs require H.264.

## Signals

Good:

- `speed=0.98x-1.02x` after warmup.
- `drop_frames=0` or rare.
- `buffered_chunks` stays near `0`.
- `tts_buffered_bytes` drains instead of growing forever.
- `tts_playback_speed` returns to `1.00` after catch-up.
- No `tts segment queue overflow` in normal healthy runs.
- YouTube shows healthy stream.

Bad:

- `speed <0.95x` for more than 30 seconds.
- `fifo_would_blocks` grows nonstop.
- `host_audio_stale_chunks_dropped`, `ready_host_bytes_dropped`, or
  `video_stale_chunks_dropped` grow during normal healthy runs.
- `tts_buffered_bytes` grows forever.
- `tts_playback_speed` stays above `1.00` for the whole run.
- `tts segment queue overflow` appears repeatedly.
- Repeated FFmpeg restarts.
- YouTube says not enough video or viewers buffer.

## Live Failure Policy

Brivva is a live-commerce streamer. Correct behavior is not "never drop
anything"; correct behavior is "keep the show live, preserve meaning, and make
every degradation visible in logs." A translated sentence arriving one minute
late can be worse than a missing sentence because price, inventory, and CTA may
already have changed.

### Normal Speech

Host speaks in short product sentences.

Policy:

- Let Soniox endpoint detection finalize naturally.
- Flush translated text after punctuation.
- Send up to three short sentences per TTS request.
- Keep TTS playback at normal speed when translated audio backlog is under 2s.

Expected user experience: translated audio is smooth and slightly delayed.

### Long Ramble / No Pause

Host talks continuously, reads a long paragraph, or counts from 1 to 100.

Risk: Soniox may wait for a pause and hold one giant utterance. One giant TTS
request returns late, then the translated stream falls behind.

Policy:

- Send Soniox manual finalize every `BRIVVA_STT_FORCE_FINALIZE_MS` of active
  audio. Default: `3000`.
- Flush translated text after three sentence boundaries.
- Force-flush translated text by length at roughly 100 chars.

Expected user experience: translation arrives in live chunks. It may sound a
little less literary, but it does not freeze for a full monologue.

### Slight TTS Backlog

TTS is 2-5s behind because ElevenLabs returned a chunk late.

Policy:

- Keep host audio/video on time.
- Drain translated audio faster until it catches up.
- Log `tts_buffered_bytes`, `tts_buffered_segments`, `tts_playback_speed`,
  and `tts_catchup_active`.

Expected user experience: translated voice gets slightly faster for a short
period, then returns to normal.

### Severe TTS Backlog

TTS is more than 10-15s behind.

Risk: even if every translated sentence is preserved, viewers hear stale product
information.

Policy:

- Prefer whole translated sentence/chunk drops over raw PCM byte drops.
- Never cut a translated word mid-audio.
- Log language, utterance/chunk id, text length, duration, and reason.

Implemented server behavior: RTMP TTS queue stores `TtsSegment` entries, catches
up by consuming translated PCM faster when backlog grows, and only drops whole
segments when the hard live cap is exceeded.

### ElevenLabs Slow Or Down

Risk: TTS request times out or returns non-2xx.

Policy:

- Drop only that TTS request.
- Keep source/original stream alive.
- Keep translated subtitles if Soniox translation succeeded.
- Log `utterance_id`, language, deadline, and provider error.

Expected user experience: translated voice may disappear briefly; original show
continues.

### Soniox Slow Or Down

Risk: no STT, no subtitles, no translated TTS.

Policy:

- Keep original stream alive.
- Reconnect Soniox up to configured retries.
- Log provider error and reconnect count.

Expected user experience: original show continues; translation temporarily stops.

### FFmpeg Or RTMP Backpressure

Risk: encoder or platform upload stalls.

Policy:

- Keep audio and video together inside the short lag window.
- Catch up by draining faster when FFmpeg accepts writes again.
- Drop only after max lag.
- For video, resume on IDR/keyframe when stale packets were dropped.
- Log exact dropped counters.

Expected user experience: short stalls recover; severe stalls may create a
visible jump instead of endless buffering.

### Too Many Outputs

Risk: GPU/CPU/network cannot handle every language/platform encoder.

Policy:

- Detect `speed < 0.95` and growing buffers.
- Prefer lowering resolution/FPS or disabling lower-priority outputs over
  letting every stream degrade.
- Long-term: use per-language encoded fanout so one encode feeds multiple RTMP
  publishers for the same language.

Expected user experience: high-priority streams stay healthy first.

### Browser Cannot Provide H.264

Risk: browser/device sends unsupported codec. RTMP platforms require H.264.

Policy:

- Server accepts H.264 only.
- Frontend requests H.264 preferred codec.
- If unavailable, fail before going live with a clear browser/device error.

Expected user experience: clear early failure instead of broken livestream.

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

### 6. Backlog Catch-Up

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

### 7. Backlog Catch-Up With All Outputs

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

### 8. One Bad Destination

Goal: one failed platform must not kill others.

```bash
MP4_FANOUT_SMOKE_RTMP_URLS=rtmp://127.0.0.1:1/live/bad
```

Run base command with normal YouTube keys too.

Pass: bad destination logs failure; valid YouTube outputs remain live.

### 9. TTS Failure

Goal: translated audio failure must not kill original stream.

Run with wrong ElevenLabs key:

```bash
ELEVENLABS_API_KEY=bad
```

Pass: source/pass streams continue. Translated streams show subtitle/TTS failure
logs, but video does not stall.

### 10. STT Failure

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
