# Brivva Scheduler Design

**Date:** April 16, 2026  
**Status:** V1 scheduler design

## Goal

One component owns playback timing.

Scheduler decides:
- what audio should play now
- what video should play now
- what is too late
- what gets dropped

Not audio thread.
Not video thread.
Not FFmpeg.

## Core Rule

For any media item:

```text
play_deadline = server_session_start + capture_ts_ms + configured_delay_ms
```

If `play_deadline <= now`, item is due.

## Responsibilities

Scheduler must:
- consume timestamped audio/video buffers
- keep source audio and source video on same timeline
- produce deterministic output cadence
- surface overload by metrics
- never hide timing bugs with burst catch-up hacks

## Non-Responsibilities

Scheduler does not:
- decode media
- translate text
- generate TTS
- reconnect RTMP
- guess missing timestamps

## Inputs

### Audio buffer

Ordered by:
- `capture_ts_ms`
- then `seq`

Each item:

```text
AudioFrame {
  seq
  capture_ts_ms
  duration_ms
  pcm
}
```

### Video buffer

Ordered by:
- `capture_ts_ms`
- then `seq`

Each item:

```text
VideoChunk {
  seq
  capture_ts_ms
  duration_ms
  is_keyframe
  chunk_kind
  bytes
}
```

## Outputs

Two sinks:

### Audio sink
- fixed cadence
- one `20ms` frame per tick

### Video sink
- variable cadence
- zero or more due chunks each tick

## Clocks

## Session Start

When valid `session_init` received:

```text
server_session_start = Instant::now()
```

Browser timestamps are session-relative.

## Delay

For V1 source stream:

```text
configured_delay_ms = e.g. 1000
```

## Scheduler Tick

Run every:

```text
10ms
```

Reason:
- audio frame is `20ms`
- 10ms tick gives margin without too much CPU wakeup

## Internal State

```text
SchedulerState {
  server_session_start: Instant,
  configured_delay_ms: u64,
  audio_buffer: RingBuffer<AudioFrame>,
  video_buffer: RingBuffer<VideoChunk>,
  last_audio_seq: u64,
  last_video_seq: u64,
  next_audio_play_ts_ms: u64,
  video_initialized: bool,
  last_video_keyframe_ts_ms: Option<u64>,
  metrics: SchedulerMetrics,
}
```

## Audio Scheduling

Audio is exact cadence path.

At each audio step:

```text
target_capture_ts_ms = next_audio_play_ts_ms
target_deadline = server_session_start + target_capture_ts_ms + delay
```

If `target_deadline <= now`, scheduler must emit one audio decision:
- play matching frame
- or silence if missing
- or drop late-arriving frame if already missed

Then:

```text
next_audio_play_ts_ms += 20
```

## Video Scheduling

Video is due-based path.

At each scheduler tick:
- find all video chunks where `play_deadline <= now`
- emit them in order
- drop chunks too late to be useful

Video does not require fixed 20ms cadence.

## Main Loop

Pseudo:

```text
loop every 10ms:
    now = Instant::now()

    emit_due_audio(now)
    emit_due_video(now)

    update_metrics(now)
```

## `emit_due_audio(now)`

Pseudo:

```text
while audio_deadline(next_audio_play_ts_ms) <= now:
    if exact audio frame for next_audio_play_ts_ms exists:
        send pcm to audio sink
        remove frame from buffer
    else:
        send 20ms silence
        metrics.audio_silence_fill += 1

    drop any audio frames older than next_audio_play_ts_ms
    next_audio_play_ts_ms += 20
```

## Exact Match Rule

V1 assumes fixed 20ms source frames.

So matching audio frame means:

```text
frame.capture_ts_ms == next_audio_play_ts_ms
```

If browser misses a frame:
- fill silence
- do not shift clock

This is important.

Clock must stay stable even when media missing.

## `emit_due_video(now)`

Pseudo:

```text
while front video chunk exists and video_deadline(front) <= now:
    if chunk too late beyond late threshold:
        drop it
        metrics.video_late_drop += 1
    else:
        send chunk to video sink
        if chunk.is_keyframe:
            last_video_keyframe_ts_ms = chunk.capture_ts_ms
        if chunk.chunk_kind == init:
            video_initialized = true

    pop chunk from buffer
```

## Drop Policy

Explicit. No hidden catch-up.

## Audio Drop Policy

### Case 1: frame on time
- play frame

### Case 2: frame missing
- play silence

### Case 3: frame arrives after its slot already passed
- drop frame
- increment `audio_late_drop`

## Video Drop Policy

### Case 1: chunk due and usable
- emit chunk

### Case 2: chunk very late
- drop chunk

### Case 3: media chunks arrive before init segment
- hold until init arrives
- or drop if init never arrives within timeout

## Late Thresholds

Initial V1:

### Audio

```text
audio_late_drop_threshold_ms = 40
```

If current slot already moved more than 40ms past frame timestamp:
- drop frame

### Video

```text
video_late_drop_threshold_ms = 100
```

If chunk due long ago:
- drop chunk

Reason:
- stale video chunk worse than clean drop

## Silence Fill Policy

Silence allowed only in audio path.

Use when:
- exact due frame missing

Silence size:
- exactly one `20ms` PCM frame

Never:
- stretch neighboring audio
- burst old audio to catch up

## No Catch-Up Bursts

Forbidden in V1:
- dumping backlog audio fast
- resetting tick anchor and pretending sync fixed
- draining many delayed audio frames in one cycle

Reason:
- causes audible glitches
- hides overload
- breaks deterministic sync

## Buffer Pruning

After each scheduler iteration:

### Audio prune

Drop all audio frames where:

```text
frame.capture_ts_ms < next_audio_play_ts_ms - 40
```

### Video prune

Drop all video chunks where:

```text
chunk.capture_ts_ms + chunk.duration_ms < video_play_cursor_ms - 100
```

## Startup Behavior

Startup should be simple.

### Audio

Do not start until:
- `session_init` received
- first audio frame exists

Set:

```text
next_audio_play_ts_ms = first_audio.capture_ts_ms
```

### Video

Do not emit media chunks until:
- init segment received

No startup realign hack.

If startup media late:
- drop late media
- metrics show startup delay

## Backpressure

If sink blocks:
- scheduler must record blocked time
- system must expose overload

Do not silently absorb.

## Sink Contracts

## Audio Sink Contract

Input:
- exactly one PCM frame per emission
- fixed 20ms frame size

Output target:
- FIFO/pipe feeding FFmpeg audio input

## Video Sink Contract

Input:
- encoded chunks in timestamp order

Output target:
- FFmpeg stdin for video path

## Metrics

Track:

```text
audio_frames_played
audio_frames_silence_filled
audio_frames_late_dropped
video_chunks_emitted
video_chunks_late_dropped
audio_sink_block_ms
video_sink_block_ms
audio_buffer_depth_ms
video_buffer_depth_ms
current_audio_play_ts_ms
current_video_play_ts_ms
startup_to_first_audio_emit_ms
startup_to_first_video_emit_ms
```

## Failure Modes

### Audio producer too slow
- silence fills rise

### Video producer too slow
- video late drops rise

### Network jitter from browser
- out-of-order and late counters rise

### FFmpeg blocked
- sink block time rises
- both audio/video lag visible

### CPU overload
- scheduler loop misses ticks
- drop counters rise

## Translation Later

When adding translated streams:
- keep source scheduler unchanged
- translated scheduler can reuse same video timeline
- translated audio segments scheduled by source utterance timestamps

Never make source scheduler wait for TTS.

## Pseudo-Code

```text
fn scheduler_tick(now):
    emit_due_audio(now)
    emit_due_video(now)
    prune_late_audio()
    prune_late_video()
    update_metrics(now)
```

```text
fn emit_due_audio(now):
    while deadline(next_audio_play_ts_ms) <= now:
        frame = audio_buffer.pop_exact(next_audio_play_ts_ms)
        if frame exists:
            audio_sink.write(frame)
            metrics.audio_frames_played += 1
        else:
            audio_sink.write(silence_20ms)
            metrics.audio_frames_silence_filled += 1

        next_audio_play_ts_ms += 20
```

```text
fn emit_due_video(now):
    while let Some(chunk) = video_buffer.front():
        if deadline(chunk.capture_ts_ms) > now:
            break

        if chunk.too_late(now):
            metrics.video_chunks_late_dropped += 1
            video_buffer.pop_front()
            continue

        video_sink.write(chunk)
        metrics.video_chunks_emitted += 1
        video_buffer.pop_front()
```

## Success Conditions

Scheduler correct if:
- source audio and video remain aligned over long run
- no burst catch-up behavior needed
- overload visible in metrics
- dropping late media preserves smoothness better than blocking
