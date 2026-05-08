# Browser WebRTC Ingest Roadmap

## Current root cause

Brivva uses browser WebRTC for host camera/video and a WebSocket PCM path for host audio. RTMP outputs then require FFmpeg to publish H.264/AAC FLV to YouTube, Grip, and other platforms.

The risky assumption was: **every supported browser can produce stable H.264 WebRTC video quickly and reliably**. That is false.

Observed failures:

- Record could stall before video reached FFmpeg because frontend waited too long for ICE gathering.
- Chrome/Brave usually produce usable H.264, but headless Brave/OpenH264 can encode slowly.
- Safari has H.264, but SDP/ICE/device behavior differs.
- Firefox often prefers VP8 and H.264 availability depends on platform/OpenH264 state.
- Server currently accepts only H.264 RTP for video; non-H.264 tracks cannot feed FFmpeg.
- UI `LIVE` has historically meant internal output process/route alive, not platform-confirmed ingest.

WebRTC itself is standard and appropriate. The incomplete part is our **codec/network/media-server compatibility layer**.

## Target architecture

```text
Browser WebRTC (H.264 / VP8 / future codecs)
  -> Brivva ingest layer with TURN support
  -> normalize to raw/encoded video frames
  -> FFmpeg/GStreamer encode H.264 + AAC
  -> RTMP/RTMPS to YouTube, Grip, TikTok, etc.
  -> provider-confirmed health back to UI
```

## Implementation phases

### Phase 1 — Reliability without architecture rewrite

- Bound ICE gathering before sending offer. Done.
- Add configurable ICE servers so production can use TURN, not STUN-only. Done for frontend (`VITE_WEBRTC_ICE_SERVERS`) and server (`BRIVVA_WEBRTC_ICE_SERVERS`).
- Remove hard Chrome/Brave dashboard gate; warn instead and rely on runtime diagnostics. Done.
- Keep detailed media diagnostics:
  - outbound codec/encoder/fps/resolution
  - first H.264 write to FFmpeg
  - FFmpeg progress / speed / bitrate
- Do not downgrade RTMPS globally. Any kill-switch must be platform-scoped and off by default.

### Phase 2 — Correct health semantics

UI states should mean:

- `Ready`: stream row exists.
- `Connecting`: WebRTC/socket negotiation active.
- `Publishing`: FFmpeg is writing bytes to RTMP. Internal FFmpeg success now emits `output.publishing`, not `output.live`.
- `Live`: platform confirms ingest / live stream health.
- `Degraded`: output running but below realtime / backpressured / missing video.
- `Failed`: provider rejects or FFmpeg cannot recover.

YouTube should use LiveStreams API `status.streamStatus` and health details. Grip should use official Seller/API or RTMP/IVS health if available.

### Phase 3 — Cross-browser codec support

Server must not require browser H.264 only.

Minimum:

- H.264 RTP depacketizer: current path.
- VP8 RTP depacketizer: required for Firefox reliability.
- FFmpeg/GStreamer transcoding path from VP8/H.264 into RTMP-safe H.264/AAC.

Preferred long-term:

- Use a media server layer: GStreamer, LiveKit, Janus, mediasoup, or Pion sidecar.
- Browser sends WebRTC using its best supported codec.
- Media layer normalizes frames and feeds encoder.

### Phase 4 — Network hardening

- Deploy coturn.
- Configure `turns:` with credentials.
- Support multiple ICE servers.
- Add connection diagnostics for relay/srflx/host candidate pair type.

## Current policy

- WebRTC is the correct browser capture protocol.
- Browser H.264-only ingest is not sufficient for equal Chrome/Brave/Firefox/Safari support.
- Production must support TURN.
- Platform `LIVE` labels must eventually be provider-confirmed, not internal-process-confirmed.
