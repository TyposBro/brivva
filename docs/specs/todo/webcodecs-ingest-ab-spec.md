# WebRTC vs WebCodecs Ingest A/B Spec

Status: implemented (code complete 2026-05-09; production soak pending)
Owner: Brivva engineering
Related:

- `docs/specs/todo/prod-e2e-automation-stress-test.md`
- `docs/specs/todo/webcodecs-production-soak-roadmap.md`
- `docs/specs/todo/webcodecs-prod-e2e-stress-test.md`
- `docs/browser-webrtc-ingest-roadmap.md`
- `docs/launch-browser-and-media-runbook.md`
- `frontend/src/features/broadcast/presentation/use-webcam.ts`
- `frontend/src/features/broadcast/presentation/use-host-session.ts`
- `server-rs/src/features/broadcast/data/session_ws/webrtc.rs`
- `server-rs/src/features/broadcast/data/session_ws/messages.rs`
- `server-rs/src/features/broadcast/data/ffmpeg/*`

## Problem

Brivva moved from simple frame push/JPEG-style ingest to browser WebRTC video ingest to reduce bandwidth and improve video quality. The bandwidth win came with production instability:

- browser codec roulette: H.264 vs VP8;
- SDP/ICE/DTLS/RTP negotiation failures;
- TURN/NAT/corporate-network behavior;
- browser FPS/settings lying;
- WebRTC RTP timestamps and FFmpeg clock mismatch;
- H.264 packetization/SPS/PPS corruption;
- hard-to-debug CloudWatch logs;
- internal `output.live` not always matching YouTube visible-live.

Brivva is not a conferencing product. It needs controlled one-way host ingest into an AWS Rust server, then deterministic RTMP fanout to YouTube/Grip/TikTok. Reliability and observability are more important than conferencing-style packet drop behavior.

## Goal

Add an **advanced frontend setting** that lets the operator choose the media ingest mode per browser/session:

```text
Auto
WebRTC hardened
WebCodecs over WebSocket experimental
```

Then run the same production E2E stress harness against both modes to decide if hardening WebRTC is good enough or if WebCodecs is better for Brivva.

## Non-goals

- Do not remove WebRTC immediately.
- Do not make WebCodecs the default until production YouTube/Grip soak passes.
- Do not claim Firefox/Zen support until real visible-live/VOD tests pass.
- Do not introduce WebTransport in the first implementation. Use WebSocket first for simpler proof; WebTransport/QUIC is Phase 2.
- Do not degrade audio STT reliability. Existing timestamped PCM audio path remains the source of truth.

## Product UX

### Advanced setting location

Add an advanced section in the host flow where Aziz/operator can toggle this before pressing Record.

Recommended placement:

1. Session setup page: `/session/:id/setup`
   - Add `Advanced media settings` accordion below voice setup / above `Go Live`.
2. Live page: `/session/:id/live`
   - Show read-only active ingest mode in diagnostics after connection.

Dashboard global settings can exist later, but per-session setup is safer because the operator can intentionally choose mode for each test.

### UI copy

Advanced media settings card:

```text
Media ingest mode

Auto (recommended)
Use Brivva's safest current production path for this browser.

WebRTC hardened
Browser sends camera video through WebRTC. Lower bandwidth, but depends on
ICE/TURN/browser codec behavior.

WebCodecs over WebSocket (experimental)
Browser encodes video frames directly and sends them through the same secure
media socket as audio. More deterministic and easier to debug, but newer.
```

Show compatibility badges:

- `Available` if required APIs are present.
- `Unavailable in this browser` if missing.
- `Experimental` for WebCodecs.
- `Using fallback: WebRTC` if Auto chooses WebRTC.

### Persistence

Persist locally, not in product session DB initially:

```text
localStorage key: brivva:mediaIngestMode
values: auto | webrtc | webcodecs_ws
```

Reason: this is an operator/debug option, not a customer-facing session field yet.

Future: add session-level `media_ingest_mode` to Workers only after the A/B proves product value.

### Safety behavior

- Default = `auto`.
- `auto` initially resolves to `webrtc` for production.
- If user chooses `webcodecs_ws` but browser lacks required APIs, disable Record and show clear copy:

```text
WebCodecs ingest is not available in this browser. Use WebRTC or switch to Chrome/Brave.
```

- If WebCodecs starts but encoder fails, fail before RTMP start if possible. Do not silently fall back mid-stream after destinations have gone live.

## High-level architecture

Current:

```text
Browser camera
  -> WebRTC RTP H.264/VP8
  -> server-rs Pion/webrtc depacketizer
  -> FFmpeg pipe
  -> RTMP platforms

Browser mic
  -> AudioWorklet PCM over WebSocket
  -> Soniox/STT + original audio mix
```

New A/B:

```text
Browser camera
  -> WebCodecs VideoEncoder
  -> encoded video chunks over authenticated media WebSocket
  -> server-rs WebCodecs chunk parser
  -> same FFmpeg manager/drain path
  -> RTMP platforms

Browser mic
  -> unchanged timestamped PCM over WebSocket
  -> Soniox/STT + original audio mix
```

Phase 2 option:

```text
WebCodecs chunks over WebTransport/QUIC
```

Only after WebSocket version proves better media behavior.

## Frontend design

### Types

Add:

```ts
export type MediaIngestMode = "auto" | "webrtc" | "webcodecs_ws";
export type ResolvedMediaIngestMode = "webrtc" | "webcodecs_ws";
```

Resolution:

```ts
function resolveMediaIngestMode(requested: MediaIngestMode): ResolvedMediaIngestMode {
  if (requested === "webrtc") return "webrtc";
  if (requested === "webcodecs_ws") return "webcodecs_ws";
  return "webrtc"; // until WebCodecs soak passes
}
```

After WebCodecs passes soak, `auto` can become:

```text
Chrome/Brave desktop: webcodecs_ws
Firefox/Zen: webcodecs_ws if supported, else webrtc VP8
Safari: webrtc or unsupported until proven
```

### Hook split

Current `useWebcam` owns camera + WebRTC. Split into:

```text
useCameraPreview
useWebRtcVideoIngest
useWebCodecsVideoIngest
useHostSession chooses active ingest
```

Minimal first step can keep `useWebcam` and add a sibling:

```text
frontend/src/features/broadcast/presentation/use-webcodecs-video.ts
```

`useHostSession.startRecording()`:

```ts
const mode = resolveMediaIngestMode(selectedMode);
await audio.current.start(...);
if (mode === "webrtc") await startFrameStreamingWebRtc();
if (mode === "webcodecs_ws") await startFrameStreamingWebCodecs();
dispatch({ type: "recording_started", analyser, mediaIngestMode: mode });
```

### WebCodecs browser capability check

Required:

```ts
typeof VideoEncoder !== "undefined"
typeof MediaStreamTrackProcessor !== "undefined" // or fallback canvas loop
typeof VideoFrame !== "undefined"
```

Optional/fallback:

- `MediaStreamTrackProcessor` for direct camera frames.
- `HTMLVideoElement.captureStream()` for prod E2E fixture shim.
- Canvas `requestVideoFrameCallback` fallback only for tests; avoid production timer throttling.

### Codec selection

Initial WebCodecs codec preference:

1. VP8 for easier cross-browser/server normalization.
2. H.264 Annex-B only if `VideoEncoder.isConfigSupported()` confirms support and server path is ready.

Reason: current server already has VP8-to-FFmpeg IVF support in the WebRTC path/tests. H.264 WebCodecs output can involve AVCC/Annex-B parameter-set details; support later after VP8 path is stable.

Candidate configs:

```ts
const vp8Config = {
  codec: "vp8",
  width: 720,
  height: 1280,
  framerate: 30,
  bitrate: 2_800_000,
  latencyMode: "realtime",
};
```

For H.264 later, feature-detect exact config and emit parameter sets explicitly.

### Frame timing

Every encoded video chunk must carry:

- `sequence` monotonically increasing;
- `capture_time_us` from `VideoFrame.timestamp` or `performance.now()` fallback;
- `duration_us` if available;
- `key_frame`;
- `codec`;
- `width`, `height`, `fps` declared config;
- `client_sent_time_us`.

The server must use these timestamps for media ordering and drift logs, not wall-clock arrival only.

### Backpressure policy

WebSocket `bufferedAmount` is observable. WebRTC hides too much.

Client policy:

- Maintain a bounded queue of encoded chunks.
- If queue exceeds `maxBufferedVideoMs`, drop old non-keyframes first.
- If queue still too large, request/force next keyframe and drop until keyframe.
- Log every drop to session logs:

```json
{
  "event": "frontend.webcodecs_video_drop",
  "reason": "ws_backpressure",
  "dropped_frames": 12,
  "buffered_amount": 1234567,
  "queue_ms": 600
}
```

Default launch thresholds:

```text
max WS bufferedAmount: 4 MB
max queued video: 500 ms
keyframe interval: 1s
fps: 30
resolution: 720x1280 portrait
bitrate: 2.5–2.8 Mbps
```

### Frontend diagnostics

Expose in live UI:

```text
Media ingest: WebCodecs over WebSocket / WebRTC
Codec: VP8 / H.264
Capture: 720x1280 @ 30fps
Encoded: 720x1280 @ 30fps
Sent frames: N
Dropped frames: N
WS buffered: N MB
Server accepted frames: N
Server latest media PTS: T
```

For WebRTC, keep existing outbound stats and add:

```text
ICE candidate pair type
TURN/relay vs srflx/host
codec selected
packet loss / jitter if browser exposes it
```

## Wire protocol

Use the existing authenticated `/api/session` WebSocket in Phase 1. It already carries JSON control and binary audio.

Current binary behavior:

- raw PCM or `BTA2` timestamped audio binary frames go to audio.

Add a small binary multiplexer. New video binary frames start with magic `BTV1`, so legacy raw PCM remains valid.

### Video start JSON

Before video chunks:

```json
{
  "type": "video:webcodecs_start",
  "mode": "webcodecs_ws",
  "codec": "vp8",
  "width": 720,
  "height": 1280,
  "fps": 30,
  "bitrate_bps": 2800000,
  "keyframe_interval_ms": 1000,
  "timebase_us": 1
}
```

Server replies:

```json
{
  "type": "video:webcodecs_ready",
  "accepted": true,
  "codec": "vp8",
  "message": "WebCodecs video ingest ready"
}
```

If rejected:

```json
{
  "type": "video:webcodecs_error",
  "message": "VP8 WebCodecs ingest is disabled on this server"
}
```

### Video binary frame

Binary layout:

```text
0..3    magic: ASCII "BTV1"
4       version: u8 = 1
5       codec: u8 1=vp8, 2=h264_annexb
6       flags: u8 bit0=keyframe, bit1=config
7       reserved: u8
8..15   sequence: u64 little-endian
16..23  capture_time_us: u64 little-endian
24..31  duration_us: u64 little-endian, 0 if unknown
32..39  client_sent_time_us: u64 little-endian
40..43  width: u32 little-endian
44..47  height: u32 little-endian
48..51  payload_len: u32 little-endian
52..    encoded payload bytes
```

Why binary header, not JSON per frame:

- lower overhead;
- easy CloudWatch-safe metadata logging;
- no base64;
- no ambiguity with audio because `BTV1` magic is impossible for raw PCM to intentionally mean video.

### Video stop JSON

```json
{ "type": "video:webcodecs_stop" }
```

Server stops video ingest for that live session without ending audio/session.

## Server design

### New modules

```text
server-rs/src/features/broadcast/data/session_ws/webcodecs.rs
server-rs/src/features/broadcast/domain/video_ingest.rs
```

### Message handling

Update `session_ws/messages.rs` / `handle_binary`:

```text
if binary starts BTV1 -> handle WebCodecs video frame
else -> existing audio path
```

Update `handle_text`:

- route `video:webcodecs_start`;
- route `video:webcodecs_stop`;
- existing `webrtc:offer` remains unchanged.

### LiveSession state

Add current video ingest state:

```rust
pub enum VideoIngestKind {
    None,
    WebRtc,
    WebCodecsWs,
}
```

Rules:

- A live session may have only one active video ingest kind.
- If WebRTC already active, reject WebCodecs start.
- If WebCodecs active, reject WebRTC offer.
- Stop closes the active mode cleanly.

### WebCodecs VP8 path

Server receives encoded VP8 chunks. It must feed the existing FFmpeg path in a format FFmpeg understands.

Implementation options:

1. Convert per-frame payloads into IVF stream bytes server-side.
2. Reuse/extend existing VP8 IVF wrapper logic from `session_ws/webrtc.rs`.
3. Push chunks into `RtmpManager` with a codec-tagged video chunk type.

Target domain type:

```rust
pub enum EncodedVideoCodec {
    H264AnnexB,
    Vp8Ivf,
}

pub struct EncodedVideoChunk {
    pub codec: EncodedVideoCodec,
    pub bytes: Vec<u8>,
    pub is_keyframe: bool,
    pub media_pts_us: u64,
    pub duration_us: Option<u64>,
    pub width: u32,
    pub height: u32,
    pub sequence: u64,
}
```

Current FFmpeg code is H.264/WebRTC-named in places. Refactor names toward generic encoded video:

```text
push_video_h264 -> push_video_chunk
video_h264_buffers -> video_buffers
```

Keep backward-compatible helpers for WebRTC H.264.

### FFmpeg input strategy

For WebCodecs VP8:

- FFmpeg input reads IVF from pipe.
- FFmpeg encodes H.264/AAC RTMP output as today.
- Output remains mobile portrait H.264 720x1280 @ 30fps.

For WebCodecs H.264 later:

- If Annex-B, feed like current H.264 path.
- If AVCC, convert to Annex-B server-side before FFmpeg.

### Server diagnostics

Log structured events:

```text
server.video_ingest_selected mode=webcodecs_ws codec=vp8 live_session_id=...
server.webcodecs_video_start codec=vp8 width=720 height=1280 fps=30 bitrate_bps=...
server.webcodecs_video_first_keyframe sequence=...
server.webcodecs_video_frame_gap expected=N got=M
server.webcodecs_video_late_drop sequence=N lag_ms=M
server.webcodecs_video_pts_drift capture_time_us=... server_elapsed_us=...
server.video_ingest_stopped mode=webcodecs_ws frames_received=N bytes_received=N drops=N
```

Need these for CloudWatch comparison with WebRTC.

## Workers / contracts

Phase 1 does not require D1 schema change.

Optional session log events are enough:

- `frontend.media_ingest_mode_selected`
- `frontend.webcodecs_start`
- `frontend.webcodecs_stats`
- `server.video_ingest_selected`
- `server.webcodecs_stats`

Future D1 fields if productized:

```sql
ALTER TABLE sessions ADD COLUMN media_ingest_mode TEXT DEFAULT 'auto';
```

Do not add this until the advanced local setting proves useful.

## Feature flags

Frontend:

```text
VITE_WEBCODECS_INGEST_ENABLED=false by default
```

Server:

```text
BRIVVA_WEBCODECS_INGEST_ENABLED=false by default
```

Both must be enabled for the UI option to be selectable.

If frontend enabled but server disabled, UI should show option disabled after media server capability check.

Capability endpoint/message options:

1. Add media WS `server:capabilities` message on connect.
2. Or add HTTP endpoint `/health/media-capabilities` on server-rs.

Suggested first response after WS open:

```json
{
  "type": "server:capabilities",
  "videoIngestModes": ["webrtc", "webcodecs_ws"],
  "webcodecsCodecs": ["vp8"]
}
```

## Advanced setting acceptance criteria

- Operator can choose `Auto`, `WebRTC hardened`, or `WebCodecs over WebSocket` before Record.
- Selection persists in `localStorage`.
- Live page shows active resolved mode.
- If unsupported, Record is blocked before RTMP publish.
- Session logs record requested and resolved mode.
- Production E2E can override mode through localStorage/env.

## A/B test plan

Use `docs/specs/todo/prod-e2e-automation-stress-test.md` harness.

Run same fixture and same YouTube destination:

```text
~/Desktop/text.mp4
```

Matrix:

```text
Chromium WebRTC 10m
Chromium WebCodecs WS 10m
Brave WebRTC 10m
Brave WebCodecs WS 10m
Firefox WebRTC 10m
Firefox WebCodecs WS 10m if supported
Zen WebRTC 10m headed/Xvfb
Zen WebCodecs WS 10m headed/Xvfb if supported
```

Then soak finalists:

```text
Brave WebRTC 30-60m
Brave WebCodecs WS 30-60m
```

Compare:

- time to Record -> first YouTube visible-live;
- setup failures;
- WebRTC ICE/TURN failures vs WebCodecs WS connect failures;
- browser capture FPS;
- encoded/output FPS;
- FFmpeg speed min/p50/p95;
- FFmpeg restart count;
- video stale drops;
- host audio stale drops;
- TTS segment overflow;
- TTS delay p50/p95;
- TTS drift ms/min;
- YouTube provider health good/active duration;
- VOD duration and frame cadence.

## WebRTC hardening checklist

Before judging WebCodecs better, harden current WebRTC enough for fair comparison:

- TURN credentials working in prod.
- ICE server list visible in diagnostics.
- Candidate pair type logged: host/srflx/relay.
- Selected codec logged: H.264/VP8.
- Browser outbound stats logged: fps, dimensions, framesSent, targetBitrate, qualityLimitationReason.
- Server logs first video packet/frame, codec, dimensions, media clock correction.
- Provider health uses YouTube confirmed live, not only FFmpeg live.
- Firefox/Zen results are separated from Chrome/Brave results.

## WebCodecs success criteria

WebCodecs WS beats WebRTC if it shows:

- fewer setup failures;
- no ICE/TURN-specific failures;
- equal or better YouTube visible-live stability;
- equal or better FPS/resolution consistency;
- fewer FFmpeg restarts/decode errors;
- clearer CloudWatch root-cause logs;
- TTS/original audio delay and drift no worse than WebRTC;
- acceptable CPU/network usage on host and AWS.

It does **not** need lower latency than WebRTC. It needs more determinism.

## Rollout plan

### Phase 0 — Spec + UI-only flag

- Add advanced setting UI.
- Persist choice.
- Show diagnostics.
- No server WebCodecs yet; option disabled behind capability flag.

### Phase 1 — WebCodecs VP8 over existing WS

- Add `BTV1` binary frame parser.
- Add VP8 IVF wrapping/server feed.
- Add frontend VideoEncoder VP8 sender.
- Keep WebRTC default.
- Run local/dev MediaMTX and unit tests.

### Phase 2 — Prod E2E A/B

- Enable for Aziz/operator only.
- Run 10m WebRTC vs WebCodecs on YouTube.
- Fix obvious bugs.
- Run 30–60m soak.

### Phase 3 — H.264 WebCodecs optional

- Add H.264 Annex-B/AVCC handling only if needed.
- Compare VP8->server transcode vs H264->server transcode.

### Phase 4 — WebTransport/QUIC optional

- Only if WebSocket HOL/backpressure becomes a measured problem.
- Keep same `EncodedVideoChunk` domain model.
- Swap transport from WS to WebTransport streams.

## Tests

Frontend unit tests:

- ingest mode localStorage read/write;
- advanced settings render and disable unsupported modes;
- WebCodecs capability detection;
- `BTV1` frame encoding layout;
- backpressure drop policy.

Frontend e2e:

- select WebRTC -> live page logs mode WebRTC;
- select WebCodecs -> live page logs mode WebCodecs if enabled;
- unsupported WebCodecs blocks Record with clear guidance.

Server unit tests:

- `BTV1` parser accepts valid frames;
- parser rejects bad magic/version/length;
- binary handler routes `BTV1` to video, raw PCM/`BTA2` to audio;
- video mode conflict rejects WebRTC after WebCodecs and vice versa;
- VP8 WebCodecs frame wraps to IVF bytes accepted by FFmpeg args path;
- sequence gaps log but do not panic.

Integration/stress:

- MP4 fixture -> WebCodecs WS -> local RTMP sink;
- one bad destination with WebCodecs mode;
- prod YouTube OAuth E2E with WebCodecs mode;
- CloudWatch summary parity with WebRTC mode.

## Open questions

- Does current Firefox/Zen expose enough WebCodecs APIs for direct camera frames, or do we need canvas/video fallback?
- Is VP8 WebCodecs output stable enough across browsers, or do we need H.264 sooner?
- Does WebSocket HOL matter at 720x1280@30 and ~2.5Mbps when audio shares the same socket?
- Should video use a second WS to isolate audio from video backpressure?
- Should `auto` ever choose WebCodecs for all browsers, or only Chrome/Brave?

## Important design warning

Do not share one unbounded WebSocket queue for audio and video without measurement. Audio feeds STT and billing-critical timing. If video backpressure delays audio frames, split transport:

```text
/api/session          JSON + audio PCM
/api/session/video    WebCodecs video chunks, same auth + session_id
```

Phase 1 can use one socket for speed, but production promotion requires proving audio delay does not worsen under video congestion.

## Implementation entrypoints

- Frontend setting/storage: `frontend/src/features/broadcast/presentation/media-ingest-mode.ts`, `media-ingest-settings.tsx`.
- Frontend WebCodecs VP8 sender + BTV1 framing: `use-webcodecs-video.ts`, `webcodecs-frame.ts`.
- Server WebCodecs VP8 ingest + IVF wrapping: `server-rs/src/features/broadcast/data/session_ws/webcodecs.rs`.
- Server generic video ingest domain state: `server-rs/src/features/broadcast/domain/video_ingest.rs`.
- Feature flags: `VITE_WEBCODECS_INGEST_ENABLED`, `BRIVVA_WEBCODECS_INGEST_ENABLED` (both default off).
- E2E override: `E2E_MEDIA_INGEST_MODE=auto|webrtc|webcodecs_ws bun run test:e2e:prod-media-stress`.
- A/B comparison: `bun run compare:e2e:media-ingest -- <out-dir> <webrtc-run-dir> <webcodecs-run-dir>`.

## Done when

- Advanced setting exists and is visible before Record.
- WebRTC and WebCodecs can be selected intentionally.
- Production E2E harness can run both modes against `~/Desktop/text.mp4`.
- `summary.json` compares both modes with objective metrics.
- At least one 30–60 min YouTube production soak exists for each candidate mode.
- Decision is recorded: keep hardening WebRTC, switch default to WebCodecs WS, or continue A/B.
