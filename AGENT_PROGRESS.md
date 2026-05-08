# Agent Progress — VP8/WebRTC Media Normalization

## Goal
Implement end-to-end VP8/WebRTC media normalization so browsers that negotiate VP8 can feed Brivva RTMP outputs instead of blackholing video.

## Checklist
- [x] Inspect current WebRTC → FFmpeg video path.
- [x] Design minimal safe codec normalization path.
- [x] Implement VP8 RTP depacketization/frame packaging.
- [x] Make FFmpeg input codec/container selectable per live session/output.
- [x] Wire WebRTC VP8 tracks into RTMP manager.
- [x] Add tests for VP8 IVF framing and FFmpeg args.
- [x] Run full frontend/backend/worker checks.
- [x] Commit stable chunk.
- [ ] Deploy backend.
- [ ] Verify production health / logs.

## Completed
- Added `VideoInputCodec` (`H264AnnexB`, `Vp8Ivf`) to FFmpeg arg builder.
- FFmpeg input can now be `-f h264` or `-f ivf` while always outputting RTMP-safe H.264/AAC.
- RtmpManager can switch input codec and restart existing outputs with preserved buffers.
- WebRTC VP8 tracks now depacketize RTP payloads, wrap frames in IVF stream/frame headers, and push them to FFmpeg.
- Video drain is codec-aware for keyframe gating (H.264 IDR vs VP8 keyframe).
- Added tests for VP8 IVF headers and FFmpeg IVF input args.

## Tests run
- `cargo check -p server-rs` — pass
- `cargo test -p server-rs webrtc --quiet` — pass
- `cargo test -p server-rs ffmpeg --quiet` — pass
- `bun run --cwd frontend typecheck` — pass
- `bun run --cwd workers typecheck` — pass
- `cargo test -p server-rs --quiet` — pass (382 unit tests; ignored manual smoke tests unchanged)

## Commits
- `2642aa8 feat: normalize VP8 WebRTC ingest`

## Blockers
- None.

## Exact next action
Deploy ECS backend with VP8 normalization and verify health/logs.
