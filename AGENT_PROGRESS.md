# Agent Progress — WebCodecs Production Soak + E2E A/B Automation

## Goal
Implement and execute WebCodecs production soak / A/B stress automation using Infisical-provided YouTube/Grip credentials.

## Checklist
- [x] Implement local runner/analyzer/wrapper automation.
- [x] Commit automation implementation.
- [x] Inspect Infisical prod secret names without printing values.
- [x] Ensure production frontend/server are deployed with WebCodecs flags enabled for explicit A/B only.
- [x] Run production WebRTC vs WebCodecs smoke with YouTube where credentials are present.
- [ ] Run Grip path if credentials/evidence become available.
- [x] Analyze smoke artifacts and record interim verdict.
- [x] Commit deploy-script fix.
- [x] Commit HTTPS fixture runner fix.
- [ ] Commit analyzer + WebCodecs startup codec hint fix.
- [ ] Deploy analyzer-independent frontend/server fix and rerun A/B.

## Completed
- Automation implementation committed: `75a4466 test(e2e): automate WebCodecs soak A/B`.
- User authorized Infisical YouTube and Grip stream keys.
- Confirmed `infisical` CLI is installed.
- Confirmed initially fetched prod frontend JS did not contain `webcodecs_ws`.
- Patched and committed deploy script env plumbing: `6a04e5b chore(deploy): expose WebCodecs flag`.
- Deployed frontend with `VITE_WEBCODECS_INGEST_ENABLED=true`; verified prod asset contains `webcodecs_ws`.
- Deployed ECS server task definition `brivva:2` with `BRIVVA_WEBCODECS_INGEST_ENABLED=1`, `BRIVVA_SESSION_LOGS=1`, `BRIVVA_VIDEO_ENCODER=nvenc`.
- Scaled GPU ECS service to 1 task.
- Infisical prod access unavailable locally due missing login/session; YouTube secrets are still reachable through deployed Workers path. Grip credentials remain unavailable locally.
- First A/B run failed before ingest because HTTPS prod page could not load `http://127.0.0.1` fixture (`loopback` PNA/CORS).
- Committed HTTPS localhost fixture runner fix: `f8a32d9 fix(e2e): serve fixture over HTTPS`.
- YouTube A/B smoke artifacts: `tmp/prod-media-ingest-ab-runs/20260509-161048-media-ingest-ab`.
- Smoke result: both modes reached YouTube live (~0.97 provider-live ratio); WebCodecs sent 3720 frames, 0 browser drops, server accepted 3660.
- Smoke failures after analyzer fix: WebRTC slow FFmpeg min speed 0.926 + ready audio drops; WebCodecs initial FFmpeg restart/exit + 42 video stale drops. Root cause for WebCodecs likely RTMP spawned as H264 then restarted when WebCodecs VP8 mode selected.
- Patched frontend/server to pass `mediaIngestMode=webcodecs_ws` on WS connect and start RTMP with VP8 input codec before FFmpeg spawn; not deployed yet.

## Remaining work
- Commit analyzer counter fix + WebCodecs startup codec hint.
- Deploy frontend/server with the startup codec hint.
- Rerun production YouTube A/B.
- Run Grip path only if local Grip credentials/API/watch evidence become available.

## Tests run
Current turn:
- `node --check scripts/prod-media-stress-e2e.mjs` — pass
- `node --check scripts/analyze-prod-media-stress.mjs` — pass
- `node scripts/analyze-prod-media-stress.mjs --self-test` — pass
- `bun run typecheck:frontend` — pass
- `bun run --cwd frontend test use-host-session` — pass (15 tests)
- `cargo test -p server-rs` — pass (389 unit tests + integration tests; 2 ignored/manual debt)
- YouTube WebRTC 10s fixture smoke (`node scripts/prod-media-stress-e2e.mjs`, Brave headless, HTTPS local fixture, strict gates off) — fixture video/audio loaded and record started; expected fail on analysis due 10s duration/provider gates.
- YouTube A/B 120s after fixture fix — completed both modes; artifacts `tmp/prod-media-ingest-ab-runs/20260509-161048-media-ingest-ab`; strict gates off for smoke.

Previous automation commit:
- `node --check scripts/prod-media-stress-e2e.mjs` — pass
- `node --check scripts/analyze-prod-media-stress.mjs` — pass
- `node --check scripts/prod-media-ingest-ab-e2e.mjs` — pass
- `node scripts/analyze-prod-media-stress.mjs --self-test` — pass
- `bun run typecheck:frontend` — pass
- `bun run typecheck:workers` — pass
- `bun run test:frontend` — pass
- `bun run test:workers` — pass
- `cargo test -p server-rs` — pass

## Commits
- `f8a32d9 fix(e2e): serve fixture over HTTPS`
- `6a04e5b chore(deploy): expose WebCodecs flag`
- `75a4466 test(e2e): automate WebCodecs soak A/B`

## Blockers
- Grip prod credentials cannot be fetched locally because `infisical run --env=prod` has no active Infisical session/login.

## Exact next action
Commit analyzer counter fix + WebCodecs startup codec hint, deploy frontend/server, then rerun YouTube production WebRTC/WebCodecs A/B.
