# E2E smoke test

One-scenario regression harness for the media pipeline. Runs nightly + on PRs
that touch `server-rs/` or this folder. Answers: **is the host-audio → RTMP
pipeline alive?**

## What it does

1. Spin up a self-contained stack via `compose.e2e.yml`:
   - `server-rs` (Fargate media service)
   - `mediamtx` (RTMP sink)
   - three Bun stubs for **workers**, **soniox**, **elevenlabs**
2. Driver (`driver.ts`) mints an HS256 JWT, opens a WS to `/api/session`,
   streams ~40s of 440Hz PCM + a JPEG every 2s.
3. After the WS closes, driver runs `ffprobe` on the mediamtx RTMP output
   and fails if audio or video tracks are missing.

Not a functional test — does not assert translation quality, caption content,
or latency. Assertion is binary: pipeline ships bytes or it doesn't.

## Stubs

| Stub | Why | What it returns |
|---|---|---|
| `workers.ts` | server-rs fetches session context at WS upgrade via `WORKERS_API_URL` | one English stream pointing at local mediamtx |
| `soniox.ts` | avoid $$ per run + offline CI | fake translation tokens every 5s |
| `elevenlabs.ts` | same | 2s silent MP3 |

Stubs share one `Dockerfile.stub` with ffmpeg preinstalled (elevenlabs uses
it to pre-render the fixture MP3 on startup).

## Local run

```bash
cd tests/e2e
docker compose -f compose.e2e.yml up --build -d
SERVER_URL=http://localhost:3000 \
WS_URL=ws://localhost:3000/api/session \
RTMP_URL=rtmp://localhost:1935/live/smoke \
JWT_SECRET=smoke-jwt-secret \
bun run smoke
docker compose -f compose.e2e.yml down -v
```

While the stack is up you can peek at the output manually:

```bash
ffplay rtmp://localhost:1935/live/smoke
```

## CI

`.github/workflows/smoke.yml` runs it nightly (09:00 UTC) + on PR paths
`server-rs/**` or `tests/e2e/**`. Red = page whoever owns the pipeline.

## Adding scenarios

Prefer widening this one scenario over adding a second — smoke stays small.
Real coverage belongs in full E2E (future, separate workflow) or unit tests.
If you must add a second scenario, extract the driver into a library first.
