# Brivva Streaming Architecture

**Date:** April 16, 2026  
**Status:** Fresh design for empty-repo rebuild

## Goal

Build stable live streaming core with one clear rule:

`play_time = capture_time + configured_delay`

Both audio and video obey same rule.

## V1 Scope

Only source stream:
- browser capture
- timestamped transport
- server buffers
- scheduler
- RTMP output

Translation comes later.

## 1. Wire Format

Use binary websocket frames.

Reason:
- lower overhead
- easier high-rate media transport
- explicit fixed header

Each websocket message:

```text
+------------+------------+-------------------+
| type (1B)  | header     | payload bytes     |
+------------+------------+-------------------+
```

## Message Types

```text
0x01 = session_init
0x02 = audio_frame
0x03 = video_chunk
0x04 = stream_end
0x05 = ping
```

## Header Encoding

All integer fields little-endian.

### `session_init`

Sent once after websocket open.

Header:

```text
u16 version
u16 flags
u32 audio_sample_rate
u16 audio_channels
u16 audio_frame_duration_ms
u16 video_timescale
u16 reserved
u64 session_start_unix_ms
```

Payload:
- UTF-8 JSON blob for codec metadata

Example JSON:

```json
{
  "video_codec": "h264",
  "video_container": "fmp4",
  "audio_codec": "pcm_s16le",
  "source_lang": "en"
}
```

### `audio_frame`

One exact frame duration each message.

Header:

```text
u64 seq
u64 capture_ts_ms
u32 duration_ms
u32 payload_size
```

Payload:
- raw PCM s16le mono 44.1kHz for V1

Rule:
- browser must send fixed-duration audio frames
- recommended V1 size: `20ms`

### `video_chunk`

Header:

```text
u64 seq
u64 capture_ts_ms
u32 duration_ms
u8  is_keyframe
u8  chunk_kind
u16 reserved
u32 payload_size
```

Payload:
- encoded video bytes

`chunk_kind`:

```text
0 = init_segment
1 = media_segment
```

Rule:
- each video chunk must carry capture timestamp for first frame represented by chunk
- browser may send variable-size chunks

### `stream_end`

Header:

```text
u32 reason_code
u32 reserved
```

No payload.

### `ping`

Header:

```text
u64 client_time_ms
```

No payload.

Use for latency measurement only. Not sync source.

## 2. Time Model

## Capture Time

Browser stamps each media unit at capture moment.

Need one monotonic clock source in browser.
Use:
- `performance.now()` for monotonic timing
- derive session-relative milliseconds

Do not use wall clock for scheduling.

## Server Time

When `session_init` arrives:
- server records `server_session_start = Instant::now()`
- browser timestamps treated as session-relative

Conversion:

```text
target_play_time = server_session_start + capture_ts_ms + configured_delay_ms
```

This gives one shared play deadline for audio and video.

## 3. Buffer Model

Two independent ring buffers:

### Audio Buffer

Store:

```text
AudioFrame {
  seq: u64,
  capture_ts_ms: u64,
  duration_ms: u32,
  pcm: Vec<u8>
}
```

### Video Buffer

Store:

```text
VideoChunk {
  seq: u64,
  capture_ts_ms: u64,
  duration_ms: u32,
  is_keyframe: bool,
  chunk_kind: Init | Media,
  bytes: Vec<u8>
}
```

## Buffer Rules

- maintain max retention window
- reject very old media
- keep ordering by `capture_ts_ms`, then `seq`
- log gaps and out-of-order arrivals

Initial retention target:
- audio: 10s
- video: 10s

## 4. Scheduler

One scheduler owns playback timing.

Not:
- separate audio timing thread with own rules
- separate video timing thread with hidden logic

Instead:
- one scheduler loop computes what is due
- scheduler feeds output sinks

## Scheduler Loop

Tick every `10ms`.

Pseudo:

```text
now = Instant::now()
play_cursor_ms = now - server_session_start - configured_delay

pop all audio frames where capture_ts_ms <= play_cursor_ms
pop all video chunks where capture_ts_ms <= play_cursor_ms

send due audio to audio sink
send due video to video sink

record late / dropped / missing metrics
```

Better exact form:

```text
play_deadline = Instant::now()

audio due if:
  server_session_start + audio.capture_ts_ms + delay <= play_deadline

video due if:
  server_session_start + video.capture_ts_ms + delay <= play_deadline
```

## Output Strategy

### Audio

Audio sink expects fixed cadence.

V1 rule:
- scheduler emits audio every `20ms`
- if exact frame due, write it
- if frame missing, write silence and count miss
- if frame too late, drop it and count late_drop

### Video

Video sink accepts variable chunks.

V1 rule:
- scheduler emits each due video chunk once
- if chunk arrives too late after play deadline, drop and count late_drop
- if chunk missing until next keyframe and decoder would break, request resync strategy later

## 5. Drop Policy

Explicit. No hidden realign hacks.

### Audio

- if frame arrives before deadline: buffer
- if frame due now: play
- if frame arrives after deadline by small amount: drop
- if frame missing entirely: emit silence

Suggested thresholds:
- late but playable window: `<= 40ms`
- after that: drop

### Video

- if chunk due now: send
- if chunk too late: drop
- if many drops since last keyframe: mark video degraded

## 6. Recovery Policy

No “catch up by dumping backlog” in V1.

Instead:
- late media gets dropped
- scheduler stays on current clock
- metrics reveal overload

Reason:
- simpler
- predictable
- no fake sync restoration
- no burst playback ugliness

## 7. FFmpeg Handoff

FFmpeg is mux/output worker.

V1 recommendation:
- one FFmpeg child per output stream
- one stdin for video
- one pipe/FIFO for audio

But timing comes from scheduler, not FFmpeg.

## Video Path

If browser already provides RTMP-compatible H.264:
- prefer `-c:v copy`

Else:
- encode once in controlled path

## Audio Path

V1:
- PCM in
- AAC out in FFmpeg

## 8. Metrics

Must expose:

```text
audio_buffer_depth_ms
video_buffer_depth_ms
audio_late_drop_count
video_late_drop_count
audio_silence_fill_count
audio_out_of_order_count
video_out_of_order_count
current_play_cursor_ms
oldest_audio_ts_ms
oldest_video_ts_ms
newest_audio_ts_ms
newest_video_ts_ms
rtmp_connected
ffmpeg_restart_count
```

## 9. Browser Rules

Browser must help stability.

### Audio

- send exact `20ms` PCM frames
- not `93ms`, not variable chunks

At 44.1kHz mono s16le:
- `20ms = 1764 bytes`

### Video

- send codec/init metadata clearly
- send chunk timestamps from same session clock as audio
- preserve ordering

## 10. Translation Extension Later

After source path proven:

Add:

```text
TranslatedAudioSegment {
  utterance_id: u64,
  source_start_ts_ms: u64,
  source_end_ts_ms: u64,
  pcm: Vec<u8>
}
```

Translated stream design:
- reuse same delayed video scheduler
- translated audio inserted by source utterance timing
- if TTS not ready by playback deadline:
  - play silence
  - or skip utterance

Do not block source stream waiting for translation.

## 11. Anti-Goals

Avoid in rebuild:
- startup realign
- jitter recovery that burst-plays backlog
- queue-depth heuristics as sync system
- host underlay before core stable
- translation coupled into source scheduler

## 12. First Build Checklist

### Browser
- [ ] session-relative monotonic timestamping
- [ ] exact 20ms audio framing
- [ ] timestamped video chunks

### Server
- [ ] binary parser
- [ ] audio/video ring buffers
- [ ] single scheduler
- [ ] deterministic drop rules
- [ ] FFmpeg output

### Validation
- [ ] source stream stable for 30+ min
- [ ] no A/V drift
- [ ] no YouTube buffering
- [ ] no periodic frame stalls
