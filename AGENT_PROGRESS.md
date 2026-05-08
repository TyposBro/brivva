# Agent Progress — Prod E2E Browser Ingest Verification

## Goal
Find what remains after VP8/WebRTC normalization and run production E2E test against `https://brivva.pages.dev` with fake media.

## Checklist
- [x] Inspect existing E2E/runbook/CDP tooling.
- [x] Ensure fake media artifacts are available.
- [x] Run prod browser E2E on `brivva.pages.dev`.
- [x] Verify frontend, backend logs, FFmpeg input codec path, and provider health where possible.
- [x] Identify and fix VP8 keyframe-gating issue found by E2E logs.
- [x] Identify and fix VP8 descriptor parser state leak found by second E2E logs.
- [ ] Commit VP8 parser fix.
- [ ] Deploy parser fix.
- [ ] Re-run prod forced-VP8 E2E after parser fix.
- [ ] Document remaining work/risks.

## Completed
- Reused headless Brave CDP session with fake media:
  - `/tmp/brivva-fake-video.y4m`
  - `/tmp/brivva-fake-audio.wav`
- Created prod session `056fd55a-bf24-4542-89a0-c67f14b829fc` on `https://brivva.pages.dev`.
- Forced browser video capabilities to remove H.264 so WebRTC negotiated VP8.
- Started Korean source + YouTube passthrough stream.
- UI confirmed YouTube provider health `stream=active health=good`, app stream `LIVE`, WebRTC outbound `720×1080 @ 30fps`.
- Backend logs confirmed VP8 path:
  - `webrtc VP8 video track started`
  - `switching ffmpeg video input codec video_input_codec=vp8_ivf`
  - FFmpeg args include `-f ivf`
  - first VP8 IVF chunk written to stdin.
- Stopped streams after tests to limit cost.
- E2E logs exposed VP8 decoder warnings after codec switch.
- Implemented stricter VP8 keyframe detection: requires VP8 keyframe bit plus `0x9d012a` sync code.
- Second E2E still showed VP8 decoder warnings; root cause found in code: reused `Vp8Packet` parser can leak optional descriptor flags between RTP packets because parser stores flags on self. Changed VP8 depacketizer to instantiate fresh `Vp8Packet` per RTP packet and added regression test.

## Tests run
- `cargo test -p server-rs ffmpeg --quiet` — pass after keyframe fix.
- `cargo test -p server-rs webrtc --quiet` — pass after parser fix.
- `cargo test -p server-rs ffmpeg --quiet` — pass after parser fix.
- Prod forced-VP8 E2E session `056fd55a-bf24-4542-89a0-c67f14b829fc` — provider live, exposed decoder warning.
- Prod forced-VP8 E2E session `deb7184d-85ce-4336-a8cd-a3a10fc3afca` — provider live, exposed parser-state issue.
- CloudWatch queries for both sessions.

## Commits
- `7353f04 fix: gate VP8 restart on true keyframes`
- Pending parser fix commit.

## Blockers
- None.

## Exact next action
Commit VP8 parser fix, deploy backend, re-run prod forced-VP8 E2E and verify no VP8 decoder warnings.
