# Brivva Wire Protocol

**Date:** April 16, 2026  
**Status:** V1 protocol draft

## Goal

Define exact bytes for browser ↔ server media transport.

Rules:
- binary websocket only
- little-endian integers
- one message per websocket frame
- timestamps session-relative, monotonic

## Frame Layout

Every websocket binary frame:

```text
+--------+-------------------+---------------+
| 1 byte | fixed header      | payload bytes |
+--------+-------------------+---------------+
| type   | depends on type   | depends type  |
+--------+-------------------+---------------+
```

## Type Table

| Type | Hex | Meaning |
|---|---:|---|
| `session_init` | `0x01` | Stream metadata and session clock start |
| `audio_frame` | `0x02` | Fixed-duration PCM audio frame |
| `video_chunk` | `0x03` | Encoded video chunk |
| `stream_end` | `0x04` | Graceful end-of-stream |
| `ping` | `0x05` | Health / latency ping |

## Common Rules

- `seq` strictly increasing per type
- `capture_ts_ms` from browser monotonic session clock
- payload size must match actual payload bytes
- unknown type = reject frame, count protocol error
- malformed frame = reject frame, do not kill session on first error

## 1. `session_init`

## Purpose

Sent once after websocket open.

Tells server:
- protocol version
- audio framing
- codec metadata

## Byte Layout

Offset after type byte.

| Offset | Size | Field | Type |
|---:|---:|---|---|
| 0 | 2 | `version` | `u16` |
| 2 | 2 | `flags` | `u16` |
| 4 | 4 | `audio_sample_rate` | `u32` |
| 8 | 2 | `audio_channels` | `u16` |
| 10 | 2 | `audio_frame_duration_ms` | `u16` |
| 12 | 2 | `video_timescale` | `u16` |
| 14 | 2 | `reserved` | `u16` |
| 16 | 8 | `session_start_unix_ms` | `u64` |
| 24 | N | `json_metadata` | UTF-8 bytes |

Fixed header size:

```text
24 bytes
```

## Metadata JSON

Example:

```json
{
  "video_codec": "h264",
  "video_container": "fmp4",
  "audio_codec": "pcm_s16le",
  "source_lang": "en"
}
```

## Flags

V1:

```text
bit 0 = audio present
bit 1 = video present
bit 2 = reserved
...
```

## Validation

- `version == 1`
- `audio_sample_rate == 44100` for V1
- `audio_channels == 1` for V1
- `audio_frame_duration_ms == 20` for V1

## 2. `audio_frame`

## Purpose

Carry one exact audio frame.

V1:
- PCM s16le
- mono
- 44.1kHz
- exactly 20ms per message

## Byte Layout

Offset after type byte.

| Offset | Size | Field | Type |
|---:|---:|---|---|
| 0 | 8 | `seq` | `u64` |
| 8 | 8 | `capture_ts_ms` | `u64` |
| 16 | 4 | `duration_ms` | `u32` |
| 20 | 4 | `payload_size` | `u32` |
| 24 | N | `pcm_payload` | raw bytes |

Fixed header size:

```text
24 bytes
```

## Payload Rules

Expected payload size for V1:

```text
44100 samples/sec * 2 bytes/sample * 1 channel * 0.020 sec = 1764 bytes
```

So:
- `duration_ms == 20`
- `payload_size == 1764`

## Validation

- `seq` > previous audio `seq`
- `capture_ts_ms` monotonic non-decreasing
- `payload_size == actual payload length`
- `payload_size == 1764` in V1

## 3. `video_chunk`

## Purpose

Carry encoded video bytes with timestamp.

## Byte Layout

Offset after type byte.

| Offset | Size | Field | Type |
|---:|---:|---|---|
| 0 | 8 | `seq` | `u64` |
| 8 | 8 | `capture_ts_ms` | `u64` |
| 16 | 4 | `duration_ms` | `u32` |
| 20 | 1 | `is_keyframe` | `u8` |
| 21 | 1 | `chunk_kind` | `u8` |
| 22 | 2 | `reserved` | `u16` |
| 24 | 4 | `payload_size` | `u32` |
| 28 | N | `chunk_payload` | raw bytes |

Fixed header size:

```text
28 bytes
```

## `chunk_kind`

| Value | Meaning |
|---:|---|
| `0` | init segment |
| `1` | media segment |

## Validation

- `seq` > previous video `seq`
- `payload_size == actual payload length`
- `chunk_kind` must be `0` or `1`
- first usable stream data must include init segment before media segments

## 4. `stream_end`

## Purpose

Graceful end.

## Byte Layout

| Offset | Size | Field | Type |
|---:|---:|---|---|
| 0 | 4 | `reason_code` | `u32` |
| 4 | 4 | `reserved` | `u32` |

Fixed header size:

```text
8 bytes
```

## Reason Codes

| Code | Meaning |
|---:|---|
| `0` | normal |
| `1` | client stop |
| `2` | device lost |
| `3` | internal error |

## 5. `ping`

## Purpose

Health and latency measurement.

## Byte Layout

| Offset | Size | Field | Type |
|---:|---:|---|---|
| 0 | 8 | `client_time_ms` | `u64` |

Fixed header size:

```text
8 bytes
```

Not timing source for playback.

## TypeScript Structs

```ts
export type SessionInit = {
  type: 0x01;
  version: number;
  flags: number;
  audioSampleRate: number;
  audioChannels: number;
  audioFrameDurationMs: number;
  videoTimescale: number;
  sessionStartUnixMs: bigint;
  metadataJson: string;
};

export type AudioFrame = {
  type: 0x02;
  seq: bigint;
  captureTsMs: bigint;
  durationMs: number;
  payload: ArrayBuffer;
};

export type VideoChunk = {
  type: 0x03;
  seq: bigint;
  captureTsMs: bigint;
  durationMs: number;
  isKeyframe: boolean;
  chunkKind: 0 | 1;
  payload: ArrayBuffer;
};

export type StreamEnd = {
  type: 0x04;
  reasonCode: number;
};

export type Ping = {
  type: 0x05;
  clientTimeMs: bigint;
};
```

## Rust Structs

```rust
#[derive(Debug)]
pub struct SessionInit {
    pub version: u16,
    pub flags: u16,
    pub audio_sample_rate: u32,
    pub audio_channels: u16,
    pub audio_frame_duration_ms: u16,
    pub video_timescale: u16,
    pub session_start_unix_ms: u64,
    pub metadata_json: String,
}

#[derive(Debug)]
pub struct AudioFrame {
    pub seq: u64,
    pub capture_ts_ms: u64,
    pub duration_ms: u32,
    pub pcm: Vec<u8>,
}

#[derive(Debug)]
pub enum ChunkKind {
    Init,
    Media,
}

#[derive(Debug)]
pub struct VideoChunk {
    pub seq: u64,
    pub capture_ts_ms: u64,
    pub duration_ms: u32,
    pub is_keyframe: bool,
    pub chunk_kind: ChunkKind,
    pub bytes: Vec<u8>,
}

#[derive(Debug)]
pub struct StreamEnd {
    pub reason_code: u32,
}

#[derive(Debug)]
pub struct Ping {
    pub client_time_ms: u64,
}
```

## Browser Encode Helpers

## Type Byte

First byte always:

```ts
view.setUint8(0, 0x02); // audio_frame example
```

## Audio Frame Encode Example

```ts
const payloadSize = pcm.byteLength;
const buf = new ArrayBuffer(1 + 24 + payloadSize);
const view = new DataView(buf);

view.setUint8(0, 0x02);
view.setBigUint64(1, seq, true);
view.setBigUint64(9, captureTsMs, true);
view.setUint32(17, 20, true);
view.setUint32(21, payloadSize, true);

new Uint8Array(buf, 25).set(new Uint8Array(pcm));
```

## Server Parse Rules

Pseudo:

```rust
let msg_type = bytes[0];
match msg_type {
    0x01 => parse_session_init(&bytes[1..]),
    0x02 => parse_audio_frame(&bytes[1..]),
    0x03 => parse_video_chunk(&bytes[1..]),
    0x04 => parse_stream_end(&bytes[1..]),
    0x05 => parse_ping(&bytes[1..]),
    _ => Err(ProtocolError::UnknownType(msg_type)),
}
```

## Parse Checks

Each parser must check:
- minimum header size
- payload size field matches actual bytes
- no integer overflow
- valid enum values

## Error Policy

### Recoverable

- one malformed media frame
- out-of-order sequence
- oversized payload

Action:
- log
- increment protocol error counter
- drop frame

### Fatal

- missing `session_init`
- unsupported protocol version
- repeated severe corruption

Action:
- close websocket

## Limits

V1 hard caps:

| Field | Limit |
|---|---:|
| audio payload | 4096 bytes |
| video chunk payload | 2 MB |
| metadata JSON | 8 KB |
| max sequence gap before warning | 10 |

## Invariants

Must stay true:
- audio frame duration fixed at 20ms
- timestamps monotonic
- browser audio/video share same clock origin
- server never invents capture timestamps

## V2 Extensions Later

Possible future:
- Opus audio
- compressed audio transport
- ack messages
- keyframe request
- translated audio segment type
- multi-track outputs

Not in V1.
