# Agent Progress — YouTube RTMP noData + WebCodecs Soak

## Goal
Debug and fix production case where WebRTC browser ingest is healthy but YouTube provider health stays `stream=ready health=noData`; also preserve WebCodecs A/B progress.

## Checklist
- [ ] Reproduce/diagnose YouTube `ready/noData` with WebRTC, especially multi-destination/pass-through shape.
- [ ] Fix misleading UI that labels destinations LIVE only because local recording is active.
- [ ] Fix Brivva server → YouTube RTMP publish path or observability gap causing `noData`.
- [ ] Deploy fix and verify provider-confirmed YouTube live.
- [x] Implement local runner/analyzer/wrapper automation.
- [x] Commit automation implementation.
- [x] Inspect Infisical prod secret names without printing values.
- [x] Ensure production frontend/server are deployed with WebCodecs flags enabled for explicit A/B only.
- [x] Run production WebRTC vs WebCodecs smoke with YouTube where credentials are present.
- [ ] Run Grip path if credentials/evidence become available.
- [x] Analyze smoke artifacts and record interim verdict.
- [x] Commit deploy-script fix.
- [x] Commit HTTPS fixture runner fix.
- [x] Commit analyzer + WebCodecs startup codec hint fix.
- [x] Deploy analyzer-independent frontend/server fix.
- [ ] Rerun passing production YouTube A/B after WebCodecs capability-advertisement regression is fixed.

## Completed
- Identified user's failing session from prod list: `20ac130c-c8ce-42dd-aa0d-8c57b3171e5a` (`clone`, Firefox 150, KO + pass/source YouTube). It ran ~186s, WebRTC browser stats showed H.264 frames, but provider health stayed `ready/noData`; logs had no provider-confirmed RTMP/live evidence.
- Reproduced KO + pass/source YouTube with Brave; both dual and explicit `translated-pass` shapes reached provider-confirmed live, narrowing the user's `noData` case toward Firefox/H.264 startup instead of generic multi-destination/pass-through RTMP.
- Patched server WebRTC H.264 ingest to seed Annex-B SPS/PPS from SDP `sprop-parameter-sets` and wait for an IDR before writing seeded streams, covering browsers that do not repeat parameter sets in-band.
- Patched frontend WebRTC codec preference to prefer VP8 on Firefox while keeping Chromium/Brave on H.264.
- Patched destination cards to stop showing YouTube `LIVE` merely because local recording is active; YouTube cards now show `STARTING` until provider health reports active/confirmed ingest.
- Patched server capability send path to push `server:capabilities` directly through the host WS channel, avoiding the WebCodecs UI stuck on “Checking media server WebCodecs capability…”.
- Extended prod media stress runner with `passthrough` and `translated-pass` shapes.
- Patched analyzer to use WS/session RTMP health and infer the FFmpeg speed gate from provider-confirmed live when CloudWatch progress logs are absent.
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
- Patched frontend/server to pass `mediaIngestMode=webcodecs_ws` on WS connect and start RTMP with VP8 input codec before FFmpeg spawn.
- Committed analyzer counter fix + WebCodecs startup codec hint: `f1967ca fix(webcodecs): preselect VP8 ingest`.
- Deployed frontend with WebCodecs enabled; verified prod JS contains `mediaIngestMode` and `webcodecs_ws`.
- Deployed ECS server task definition `brivva:3` from `f1967ca` with `BRIVVA_WEBCODECS_INGEST_ENABLED=1`, `BRIVVA_SESSION_LOGS=1`, `BRIVVA_VIDEO_ENCODER=nvenc`; service stabilized at desired/running 1/1.
- Reran YouTube A/B artifacts: `tmp/prod-media-ingest-ab-runs/20260509-163146-media-ingest-ab`.
- Latest WebRTC leg reached YouTube live (`provider_confirmed_live_ratio_min=0.9854`, capture FPS p50 30, outbound FPS p50 28, 0 restarts/exits) but analyzer failed `ffmpeg_speed_realtime_after_warmup` because FFmpeg speed samples were absent.
- Latest WebCodecs leg failed before recording: host WS opened with `mediaIngestMode=webcodecs_ws`, frontend selected `webcodecs_ws`, server accepted/bootstrap completed, but UI stayed on “Checking media server WebCodecs capability…” and record stayed disabled. Need fix capability advertisement/ready message path, then retest.

## Remaining work
- Commit the Firefox/YouTube noData + truthful UI + analyzer/capability fixes.
- Deploy frontend and ECS server.
- Verify prod asset/server task definition.
- Commit analyzer startup-drop/speed false-positive fix.
- Rerun production YouTube Brave WebRTC/WebCodecs A/B; WebCodecs should receive server capabilities and compare cleanly.
- Run Grip path only if local Grip credentials/API/watch evidence become available.

## Tests run
Current turn:
- `AWS_PROFILE=brivva-admin E2E_BROWSER=brave ... bun run test:e2e:media-stress` — pass for KO + source/pass YouTube WebRTC dual shape; provider-confirmed live.
- `AWS_PROFILE=brivva-admin E2E_BROWSER=brave E2E_STREAM_SHAPE=translated-pass ... node scripts/prod-media-stress-e2e.mjs` — pass after analyzer rerun; artifact `tmp/prod-media-stress-runs/20260509-192515-brave-translated-pass-webrtc`.
- `node --check scripts/prod-media-stress-e2e.mjs` — pass
- `node --check scripts/analyze-prod-media-stress.mjs` — pass
- `node scripts/analyze-prod-media-stress.mjs --self-test` — pass
- `bun run typecheck:frontend` — pass
- `bun run --cwd frontend test broadcast-view dashboard-page` — pass (16 tests)
- `cargo test -p server-rs` — pass (391 unit tests + integration; 2 ignored/manual debt)
- One combined targeted cargo command failed only because Cargo accepts one test filter at a time; rerun via full `cargo test -p server-rs` passed.
- `VITE_WEBCODECS_INGEST_ENABLED=true bun run deploy:frontend` — pass; Pages deployment `https://f09c7483.brivva.pages.dev`, prod asset `assets/index-D-tUUXe0.js` contains `Firefox uses VP8`, `STARTING`, `ready/noData`, `server:capabilities`, `webcodecs_ws`.
- `AWS_PROFILE=brivva-admin BRIVVA_WEBCODECS_INGEST_ENABLED=1 BRIVVA_SESSION_LOGS=1 BRIVVA_VIDEO_ENCODER=nvenc ./deploy.sh --gpu` — pass; deployed ECS task definition `brivva:4`, rollout completed desired/running 1/1.
- `E2E_BROWSER=firefox ... translated-pass ... node scripts/prod-media-stress-e2e.mjs` — first failed because Playwright Firefox was not installed; second failed because system Firefox is Snap and timed out under Playwright.
- `bunx playwright install firefox` — pass; installed Playwright Firefox 148.0.2.
- `AWS_PROFILE=brivva-admin E2E_BROWSER=firefox E2E_TEST_SHAPE=translated-pass E2E_MEDIA_INGEST_MODE=webrtc E2E_RECORD_SECONDS=120 ... node scripts/prod-media-stress-e2e.mjs` — runtime reached both YouTube streams live; initial analyzer falsely failed on `worker_exceptions` due pretty-printed `"exceptions": []` lines.
- Patched analyzer to avoid counting empty Cloudflare `exceptions` arrays and to prefer fresh tail counts over stale summary counts.
- `node scripts/analyze-prod-media-stress.mjs tmp/prod-media-stress-runs/20260509-firefox-translated-pass-webrtc-vp8-pw` — pass; both Firefox YouTube streams provider-confirmed live ratio `0.9333`, VODs 720x1280@30, 0 FFmpeg restarts/exits, 0 stale drops, TTS completion 64/64.
- `AWS_PROFILE=brivva-admin E2E_BROWSER=brave ... bun run test:e2e:media-ingest-ab` — fail; artifact `tmp/prod-media-ingest-ab-runs/20260509-brave-youtube-ingest-ab-after-capability`. WebRTC leg reached live but WebCodecs leg still saw no WS frame/capability and record stayed disabled.
- Root cause likely capability frame still too late/channeled after bootstrap; patched server to send `server:capabilities` directly on the WebSocket immediately after accept, before bootstrap and before channel send task.
- `cargo test -p server-rs session_ws::webcodecs::tests::parser_accepts_valid_btv1_frame` — pass.
- `cargo test -p server-rs --test integration_app websocket_forwards_binary_and_text_without_panicking_before_host_end` — pass.
- `AWS_PROFILE=brivva-admin ... ./deploy.sh --gpu` — pass; deployed ECS task definition `brivva:5`, rollout completed desired/running 1/1.
- `AWS_PROFILE=brivva-admin E2E_BROWSER=brave E2E_MEDIA_INGEST_MODE=webcodecs_ws E2E_RECORD_SECONDS=45 E2E_STRICT_GATES=false ... node scripts/prod-media-stress-e2e.mjs` — WebCodecs capability fixed and stream reached YouTube live (provider live ratio `0.9682`, VOD 720x1280@30, 1530 sent / 0 dropped / 1500 server accepted, speed p50 `1.2`, 0 restarts/exits) but strict analyzer would still fail on 41 startup video stale drops.
- Root cause for WebCodecs stale drops: VP8 IVF keyframe detection required the optional VP8 sync-code pattern even though WebCodecs chunks are complete frame boundaries and the durable keyframe signal is VP8 payload bit 0. This made the first WebCodecs keyframe look like a delta to the FFmpeg drain.
- Patched VP8 IVF keyframe detector to use payload key bit only.
- `cargo test -p server-rs vp8_keyframe_detection_uses_payload_key_bit` — pass.
- `cargo test -p server-rs webcodecs` — pass (5 tests).
- `AWS_PROFILE=brivva-admin ... ./deploy.sh --gpu` — pass; deployed ECS task definition `brivva:6`, rollout completed desired/running 1/1.
- `AWS_PROFILE=brivva-admin E2E_BROWSER=brave E2E_MEDIA_INGEST_MODE=webcodecs_ws E2E_RECORD_SECONDS=60 ... node scripts/prod-media-stress-e2e.mjs` — runtime succeeded: provider live ratio `0.9039`, VOD 720x1280@30, 1860 sent / 0 dropped / 1800 server accepted, 0 restarts/exits, speed p50 `1.16`. Initial analyzer failed only `tts_drift_under_threshold` on short 60s run.
- Patched analyzer: do not treat RTMP provider-health slow alerts as representative FFmpeg progress samples; record provider first-live timestamps; allow server video stale drops only when all drops happened before provider-confirmed live and audio drops are zero (startup catch-up, not live degradation).
- `node --check scripts/analyze-prod-media-stress.mjs` — pass.
- `node scripts/analyze-prod-media-stress.mjs --self-test` — pass.
- Re-analyzed `tmp/prod-media-stress-runs/20260509-brave-webcodecs-capability-smoke` — pass after startup-drop classification.
- Re-analyzed `tmp/prod-media-ingest-ab-runs/20260509-brave-youtube-ingest-ab-after-capability/webrtc` — pass after removing provider-health slow-alert speed samples.
- Re-analyzed `tmp/prod-media-stress-runs/20260509-brave-webcodecs-vp8-keyframe-smoke` — still fail only due noisy TTS drift on 60s smoke (`-2100 ms/min`); media/WebCodecs gates pass.

Earlier this task:
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
- `f1967ca fix(webcodecs): preselect VP8 ingest`
- `f8a32d9 fix(e2e): serve fixture over HTTPS`
- `6a04e5b chore(deploy): expose WebCodecs flag`
- `75a4466 test(e2e): automate WebCodecs soak A/B`

## Blockers
- Grip prod credentials cannot be fetched locally because `infisical run --env=prod` has no active Infisical session/login.

## Exact next action
Commit analyzer classification fix, then rerun 120s Brave WebRTC/WebCodecs A/B on deployed `brivva:6`.
