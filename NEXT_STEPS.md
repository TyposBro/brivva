# Brivva Next Steps

Last updated: 2026-04-29

## Current baseline

- Local Brave/Chromium live path is stable enough for demo.
- Prod was healthy before the latest local launch-hardening commits.
- Local `main` is ahead of `origin/main` with launch media fixes not yet deployed.
- Recently completed and removed from active TODOs:
  - keyframe-aware H.264 stale recovery
  - AudioWorklet mic PCM path
  - adaptive camera/video profile selection for 1080p30, 4K60, and 720p15 hosts
  - RTMP secret redaction and live media diagnostics

## Immediate next step

1. Push/deploy the three local launch-hardening commits.
2. Run one real prod `En → Ko` MacBook live test.
3. Inspect prod/server logs for:
   - browser media stats: source/outbound FPS and resolution
   - selected FFmpeg video profile
   - FFmpeg speed, dropped frames, duplicate frames, bitrate
   - video chunks written/buffered
   - audio FIFO would-block/backpressure
   - RTMP/YouTube errors

## Highest ROI remaining code work

1. Production media alerts/runbook checks.
   - Emit explicit warnings when FFmpeg encode speed stays below realtime.
   - Emit explicit warnings when FFmpeg reports dropped output frames.
   - Keep these greppable in CloudWatch during launch tests.

## Production architecture direction

2. Move from raw H.264 stdin guessing to timestamp-preserving media ingest.
   - Preserve RTP timestamps / PTS.
   - Add jitter buffer and media clock authority.
   - Decode/normalize video to stable 1080p30 first.
   - Later evaluate 1080p60/4K.

3. Encode shared video once, then mux per-language audio.
   - Current per-output FFmpeg encoding is transitional.
   - Production should avoid N video encodes for N languages/platforms.

4. Unify ingest around WebRTC A/V or another timestamped transport.
   - Current split path is:
     - video: WebRTC
     - audio: WebSocket PCM
   - Long-term premium path should carry timestamped A/V together or explicitly synchronize both clocks.

## Human / ops follow-ups

- Real fixture captures for Soniox + YouTube require live creds/audio.
- 30-minute post-merge log audit requires a real live session.
- Camera choice remains a product/demo decision if MacBook capture is visibly worse than desired.

## Not doing next

- Do not jump to GPU/NVENC as primary fix unless logs show encode `speed < 1.0x` from CPU saturation.
- Do not reintroduce canvas capture for the live uplink.
- Do not rely on `BRIVVA_H264_INPUT_FPS` as architecture; it is only a fallback/debug knob.
