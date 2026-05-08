# Agent Progress — TURN Enablement

## Goal
Enable TURN end-to-end using AWS CLI/Wrangler auth without manual intervention.

## Checklist
- [x] Discover ECS GPU host public IP and security group.
- [x] Generate TURN static credential for launch.
- [x] Open TURN security group rules.
- [x] Register ECS task definition with coturn sidecar.
- [x] Deploy ECS service and verify health.
- [x] Deploy frontend with `VITE_WEBRTC_ICE_SERVERS` pointing at TURN.
- [x] Run/verify prod WebRTC E2E over forced relay.
- [x] Rotate TURN credential after accidental tool-output exposure and redeploy backend/frontend.
- [x] Commit stable progress.

## Completed
- ECS host: EC2 `i-0d0bfc2e3131e5e92`, public IP `52.203.38.225`, SG `sg-07b1bbf305a3ff24e`.
- Opened SG ingress:
  - UDP `3478` from `0.0.0.0/0`;
  - TCP `3478` from `0.0.0.0/0`;
  - UDP relay `49152-49200` from `0.0.0.0/0`.
- Added `coturn/coturn:4.6` sidecar to ECS host-network task definition.
- Deployed ECS task definitions:
  - `brivva:69` initial TURN;
  - `brivva:70` rotated TURN credential.
- Deployed frontend twice with `VITE_WEBRTC_ICE_SERVERS`:
  - initial TURN Pages: `https://c72780b7.brivva.pages.dev`;
  - rotated TURN Pages: `https://c39fc2d9.brivva.pages.dev`.
- Forced relay E2E on prod by monkey-patching browser `RTCPeerConnection` to `iceTransportPolicy: "relay"`.
- Final relay E2E session: `b2e7ccc3-2c57-425e-911e-87bc59e81f00`.
- Final E2E confirmed:
  - app used TURN URLs and had credential present;
  - YouTube provider health: `stream=active health=good`;
  - UI: `YouTube LIVE`;
  - backend ICE connected;
  - coturn logs showed allocation count increments;
  - WebRTC outbound continued via relay.
- Stopped test streams after verification.

## Tests run
- `curl -fsS https://brivva.spiko.uz/health` — pass after TURN deployments.
- `bun run --cwd frontend build` with `VITE_WEBRTC_ICE_SERVERS` — pass.
- `bunx wrangler pages deploy frontend/dist --project-name brivva` — pass.
- `AWS_PROFILE=personal aws ecs wait services-stable --cluster brivva --services brivva --region us-east-1` — pass.
- Prod forced-relay E2E against `https://brivva.pages.dev` — pass.
- CloudWatch verification:
  - coturn started;
  - `Global turn allocation count incremented`;
  - `ICE connection state changed: connected`.

## Commits
- Pending progress commit.

## Blockers
- None.

## Exact next action
Commit this progress file. Optional future hardening: replace static frontend TURN credential with ephemeral TURN credentials endpoint and DNS hostname (`turn.brivva...`) instead of raw EC2 IP.
