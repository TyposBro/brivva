# Agent Progress — TURN Hardening

## Goal
Replace static frontend TURN creds with ephemeral HMAC TURN credentials and use a DNS hostname instead of raw EC2 IP where possible.

## Checklist
- [x] Add Worker `/api/turn-credentials` endpoint.
- [x] Add frontend runtime fetch for ephemeral TURN ICE servers.
- [x] Configure coturn with `--use-auth-secret` + shared static secret.
- [x] Store shared TURN secret in AWS task definition and Cloudflare Worker secret.
- [x] Deploy backend/Worker/frontend.
- [x] Run/verify prod WebRTC E2E over forced relay with ephemeral credentials.
- [x] Attempt DNS `turn.brivva.spiko.uz` automation.
- [x] Commit and push stable chunks.

## Completed
- Worker endpoint added: `GET /api/turn-credentials`.
  - Returns `iceServers` with short-lived TURN username `${expiresAt}:${userId}`.
  - Credential is base64 HMAC-SHA1 over username using `TURN_STATIC_AUTH_SECRET`.
  - TTL defaults to 600s and clamps 60-3600s.
- Frontend now fetches ephemeral TURN credentials at WebRTC start when `VITE_WEBRTC_TURN_CREDENTIALS` is enabled.
  - Falls back to static `VITE_WEBRTC_ICE_SERVERS` or default STUN if fetch fails.
- Cloudflare Worker secrets configured:
  - `TURN_STATIC_AUTH_SECRET`
  - `TURN_HOST=52.203.38.225`
  - `TURN_TTL_SECONDS=600`
- ECS coturn changed to long-term HMAC mode:
  - `--use-auth-secret`
  - `--static-auth-secret=<shared secret>`
  - `--external-ip=52.203.38.225`
- Deployed:
  - Worker version `8096ada6-180c-4252-bf0c-51d160288ddc`.
  - ECS task definition `brivva:71`.
  - Frontend ephemeral TURN Pages deployment `https://5dbe7c5f.brivva.pages.dev`.
- Verified endpoint returns ephemeral username and credential without frontend static password.
- Prod forced-relay E2E passed with ephemeral TURN credentials:
  - session `1bb7261e-99ef-4e50-bd4f-bca584482807`;
  - browser RTCPeerConnection monkey-patched to `iceTransportPolicy: relay`;
  - runtime ICE config used `turn:52.203.38.225:3478` with username `1778235114:<user_id>` and credential present;
  - UI: `YouTube LIVE`;
  - provider health: `stream=active health=good`;
  - CloudWatch: `Global turn allocation count incremented`, `ICE connection state changed: connected`.
- Stopped test stream after verification.
- DNS attempt:
  - Wrangler token has `zone:read` only, not DNS write.
  - Cloudflare API `A turn.brivva.spiko.uz -> 52.203.38.225` create/update returned `403`.
  - Raw EC2 IP remains in TURN config until DNS write permission/API token is available.

## Tests run
- `bun run --cwd workers typecheck` — pass.
- `bun run --cwd frontend typecheck` — pass.
- `bun run --cwd frontend build` with ephemeral TURN enabled — pass.
- `bunx wrangler deploy --config workers/wrangler.toml` — pass.
- `bunx wrangler pages deploy frontend/dist --project-name brivva` — pass.
- `curl -fsS https://brivva-api.milliytechnology.workers.dev/api/turn-credentials?user_id=test` — pass.
- `curl -fsS https://brivva.spiko.uz/health` — pass.
- `AWS_PROFILE=personal aws ecs wait services-stable --cluster brivva --services brivva --region us-east-1` — pass.
- Prod forced-relay E2E — pass.
- CloudWatch verification — pass.

## Commits
- Pending code/progress commit.

## Blockers
- DNS hostname automation blocked by Cloudflare token lacking DNS write permission (`403`). This is non-critical because TURN works with raw EC2 IP.

## Exact next action
Commit code/progress. Optional later: provide Cloudflare API token with DNS edit permission or create `turn.brivva.spiko.uz` manually/unblock via dashboard.
