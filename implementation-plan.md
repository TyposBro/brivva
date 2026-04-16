# Brivva Implementation Plan

**Date:** April 16, 2026  
**Status:** V1 rebuild execution plan

## Goal

Turn design docs into build order.

First target:
- one stable source RTMP stream
- delayed source audio/video
- no buffering
- no drift

## Build Strategy

Work in narrow vertical slices.

Rule:
- each milestone must leave system testable
- do not start translation before source path proven

## Proposed Repo Shape

```text
brivva/
├── frontend/
│   ├── src/
│   │   ├── capture/
│   │   │   ├── session-clock.ts
│   │   │   ├── audio-capture.ts
│   │   │   ├── video-capture.ts
│   │   │   └── ws-sender.ts
│   │   ├── protocol/
│   │   │   ├── encode.ts
│   │   │   └── types.ts
│   │   └── app/
│   └── package.json
│
├── server/
│   ├── src/
│   │   ├── protocol/
│   │   │   ├── mod.rs
│   │   │   ├── parser.rs
│   │   │   ├── types.rs
│   │   │   └── errors.rs
│   │   ├── ingest/
│   │   │   ├── mod.rs
│   │   │   ├── ws_handler.rs
│   │   │   ├── audio_buffer.rs
│   │   │   └── video_buffer.rs
│   │   ├── scheduler/
│   │   │   ├── mod.rs
│   │   │   ├── clock.rs
│   │   │   ├── loop.rs
│   │   │   ├── audio.rs
│   │   │   ├── video.rs
│   │   │   └── metrics.rs
│   │   ├── output/
│   │   │   ├── mod.rs
│   │   │   ├── ffmpeg.rs
│   │   │   ├── audio_sink.rs
│   │   │   └── video_sink.rs
│   │   ├── session/
│   │   │   ├── mod.rs
│   │   │   ├── state.rs
│   │   │   └── manager.rs
│   │   ├── metrics/
│   │   │   └── mod.rs
│   │   ├── main.rs
│   │   └── lib.rs
│   └── Cargo.toml
│
├── context.md
├── plan.md
├── architecture.md
├── protocol.md
├── scheduler.md
└── implementation-plan.md
```

## Principles For Module Order

Build in this order:

1. protocol
2. browser sender
3. server parser
4. buffers
5. scheduler
6. FFmpeg output
7. source-only e2e test
8. translation later

Reason:
- cannot schedule before timestamp contract exists
- cannot output before scheduler stable

## Milestone 1: Protocol Foundation

## Deliverables

### Frontend
- `protocol/types.ts`
- `protocol/encode.ts`

### Server
- `protocol/types.rs`
- `protocol/parser.rs`
- protocol tests

## Done When

- browser can encode `session_init`
- browser can encode `audio_frame`
- browser can encode `video_chunk`
- server can parse all of them
- malformed frames rejected cleanly

## Milestone 2: Browser Timestamped Capture

## Deliverables

### `session-clock.ts`
- one monotonic session-relative clock
- `nowMs(): bigint | number`

### `audio-capture.ts`
- exact `20ms` PCM framing
- no variable packet sizes

### `video-capture.ts`
- timestamp each chunk from same clock
- preserve init/media distinction

### `ws-sender.ts`
- websocket transport
- send `session_init`
- send media frames

## Done When

- audio frames always `20ms`
- audio and video timestamps use same origin
- sequence numbers monotonic

## Milestone 3: Server Ingest

## Deliverables

### `ws_handler.rs`
- accept websocket
- require `session_init` first
- route media frames

### `audio_buffer.rs`
- insert ordered audio frames
- reject invalid or too-old frames

### `video_buffer.rs`
- insert ordered video chunks
- preserve init/media order

## Done When

- server receives timestamped media
- metrics show queue depth
- out-of-order frames logged

## Milestone 4: Source Scheduler

## Deliverables

### `scheduler/clock.rs`
- hold `server_session_start`
- compute play deadlines

### `scheduler/audio.rs`
- exact-slot audio emission
- silence fill when missing
- late drop when stale

### `scheduler/video.rs`
- emit due chunks
- drop stale chunks

### `scheduler/loop.rs`
- 10ms scheduler loop
- call audio/video emission

### `scheduler/metrics.rs`
- counters and gauges

## Done When

- scheduler runs without FFmpeg first
- test sinks receive correct timing decisions

## Milestone 5: FFmpeg Output

## Deliverables

### `output/ffmpeg.rs`
- start child process
- prepare inputs
- stderr reader

### `output/audio_sink.rs`
- fixed cadence PCM writes

### `output/video_sink.rs`
- ordered encoded chunk writes

## V1 Rule

Prefer:
- `-c:v copy` when input already valid H.264
- AAC encode for audio

## Done When

- local RTMP target receives source stream
- source stream stable for short run

## Milestone 6: Source Endurance Test

## Test Scenario

- run 30+ min
- single source stream
- no translation
- normal talking + pauses

## Measure

- no YouTube buffering
- no A/V drift
- no disconnect
- no queue explosion
- no periodic pauses

## Exit Criteria

If this fails:
- do not add translation
- fix source path first

## Milestone 7: Translation Foundation

Only after Milestone 6 passes.

## Deliverables

### New server modules
- `translation/stt.rs`
- `translation/tts.rs`
- `translation/alignment.rs`

### New data type

```text
TranslatedAudioSegment {
  utterance_id
  source_start_ts_ms
  source_end_ts_ms
  pcm
}
```

## Done When

- translated audio exists as timestamped segments
- no host underlay yet

## Milestone 8: Translated Stream

## Deliverables

- translated stream reuses same delayed video timeline
- translated audio inserted by utterance timing
- silence or skip if TTS not ready by deadline

## Done When

- source stream still stable
- translated stream continuous enough

## Milestone 9: Optional Features

Only after source and translated streams both stable.

Possible:
- host underlay
- subtitles
- multi-destination fanout
- restart logic
- health UI

## Work Sequence Inside Each Milestone

For each milestone:

1. define types
2. write unit tests
3. implement narrow path
4. add instrumentation
5. manual test
6. only then move on

## Required Test Layers

## Unit Tests
- protocol encode/decode
- buffer ordering
- deadline math
- silence fill logic
- stale drop logic

## Integration Tests
- browser sender → server parser
- scheduler → fake sinks
- FFmpeg process startup

## Manual / Endurance
- long YouTube RTMP run
- talk/pause/talk pattern
- observe drift and buffering

## First Real Coding Sequence

If starting now, first five code tasks:

1. create `protocol.md`-matching TS types + encoder
2. create Rust parser + protocol structs
3. replace browser audio capture with exact `20ms` frames
4. implement server audio/video ring buffers
5. implement fake scheduler with log-only sinks

## Stop Conditions

Stop and redesign if:
- browser cannot produce exact 20ms audio frames reliably
- browser video timestamps cannot share same clock source
- FFmpeg copy path incompatible with browser video output

## Success Definition

Project on track if:
- source-only stream works long time with no buffering
- metrics explain behavior
- translation can be added without touching source scheduler core
