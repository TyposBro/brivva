# Brivva Rebuild Context

**Date:** April 16, 2026  
**Mode:** Reset  
**Assumption:** Empty repo. No legacy code constraints.

## Product Goal

Brivva = real-time multilingual live streaming system.

Host speaks on camera. System outputs:
- source-language RTMP stream
- translated RTMP streams

But rebuild order matters:
- first make source stream stable
- then add translated streams

## Current Decision

Do not continue patching old streaming core.

Reason:
- too many timing hacks
- audio and video used different scheduling models
- buffering and drift became hard to reason about
- new features stacked on unstable base

So context now assumes:
- old implementation discarded
- new implementation designed from first principles

## Core Technical Goal

Single shared media timeline.

Both source audio and source video must obey:

`play_time = capture_time + configured_delay`

Not:
- audio FIFO with video timestamps
- startup buffering hacks
- special-case recovery as primary sync method

## V1 Scope

Build only enough to prove stable source streaming:
- browser capture
- timestamped transport
- server ingest
- audio/video buffering
- deterministic scheduler
- RTMP output

Skip for now:
- translation
- TTS underlay
- subtitles
- voice cloning
- multi-destination complexity
- infra/billing/dashboard

## Required Properties

### Source stream
- stable video FPS
- stable bitrate
- no visible buffering on platform
- no frequent RTMP disconnects
- no growing A/V drift
- predictable configured delay

### Translation later
- must consume same delayed video timeline
- must not break source stream if STT/TTS slow or fail

## Recommended Architecture

## 1. Browser Sender

Browser sends two timestamped feeds:

- `audio_frame`
  - PCM or encoded audio
  - capture timestamp
  - sequence number

- `video_chunk`
  - encoded video chunk
  - capture timestamp
  - keyframe/init metadata if needed
  - sequence number

Browser should not decide playback timing.
Browser only captures and timestamps.

## 2. Server Ingest

Server responsibilities:
- receive websocket messages
- validate timestamps and ordering
- push into ring buffers
- measure late/early arrival

Data structures:
- audio ring buffer
- video ring buffer
- monotonic session clock

## 3. Scheduler

One scheduler decides what should play now.

For each output stream:
- choose target wall-clock playback time
- read media whose `capture_ts + delay <= now`
- emit synchronized audio/video

This scheduler owns sync.
Not drain threads guessing independently.

## 4. RTMP Output

FFmpeg should be dumb mux/output layer.

Prefer:
- minimal transcoding
- passthrough when source already compatible
- explicit CPU budgeting

Do not rely on FFmpeg stalls to shape timing.
Timing must come from scheduler.

## Translation Later

When source path stable:

1. STT converts source speech to text
2. translation creates target text
3. TTS generates target audio chunks
4. translated output stream uses:
   - same delayed source video timeline
   - translated audio aligned to utterance timing

First translated version should be simple:
- no host underlay
- no mixed bilingual audio
- translated TTS only

## Non-Goals Right Now

Ignore for rebuild start:
- old file layout
- old queue semantics
- old jitter recovery behavior
- old FFmpeg thread model
- old Tauri assumptions
- old SaaS migration docs

These may come back later, but not as constraints.

## Measurement First

New system must expose:
- audio queue depth
- video queue depth
- oldest buffered timestamp
- current playback timestamp
- drift between scheduled audio/video
- dropped frames/chunks count
- RTMP send health

If not measurable, not controllable.

## Design Rules

1. Source path sacred.
- never let translation logic destabilize source output

2. One sync owner.
- one place decides playback timing

3. Recovery simple.
- on overload, drop late media explicitly
- do not hide overload behind complex catch-up hacks

4. Start narrow.
- one output first
- then many outputs

5. Long-run stability > feature count.

## Immediate Next Step

Write clean technical blueprint for:
- message format
- timing model
- buffer model
- scheduler loop
- FFmpeg handoff

Then implement source-only path.
