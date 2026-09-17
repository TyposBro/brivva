# Brivva

Real-time multilingual live commerce: a host broadcasts once and the stream reaches
several markets at the same time, keeping the host's own voice.

    live stream in  ->  transcribe  ->  translate  ->  re-voice  ->  fan out to platforms

## Status: archived, not maintained

Brivva was built between March and May 2026. Development stopped because the
funding round did not close. This repository is published as a reference
implementation and a record of the engineering work. Nothing here is production
ready, no support is offered, and issues are not monitored.

## What it did

A seller running a live sale on one platform could reach non-Korean and
non-English buyers without hiring a second host or running a second broadcast.
The pipeline ingested the live feed, transcribed the host, translated the
transcript, synthesized speech in the host's cloned voice, and pushed a
separate stream to each destination platform.

Provider work in the codebase: RTMP and GRIP ingest, YouTube, TikTok and
Instagram egress, ElevenLabs for voice, plus Whisper and Deepgram for speech to
text. Per-platform RTMP egress is a first-class concept, because each destination
has its own key, its own failure mode and its own retry budget.

## Demo

A recorded end-to-end test from 16 April 2026: a real host presenting to camera,
pushed through the Brivva pipeline, with the translated output going out as a
normal YouTube live stream. Roughly 16 minutes; the host segment starts about 20
seconds in.

This is test footage recorded during development, not a product demo. The
translation quality you hear is what the pipeline produced on that day with the
providers named above.

https://www.youtube.com/watch?v=1BaK5CdfYc8

## Layout

| Path | What it is |
| --- | --- |
| `server-rs` | Rust media and pipeline service. Axum with WebSockets, Tokio, ~21k lines |
| `workers` | Cloudflare Workers product API, `brivva-api`. Hono, Drizzle, JOSE |
| `frontend` | React + React Router + Tailwind, typed from the OpenAPI contract |
| `contracts` | Shared contract package: Zod schemas, exported OpenAPI, generated Rust checks |
| `infra` | OpenTofu/Terraform: cluster, task definitions, secrets, monitoring, GPU base images |
| `docs` | Specs, the launch runbook, the stress-test suite, and the production report |
| `tests` | End-to-end tests against real providers |

Both `server-rs` and `workers` use the same internal shape: `core`, `features`,
`orchestration`. Vertical slices, one feature per directory.

## The parts worth reading

- `docs/2026.05.06-report.md` - what actually happened when we ran production
  end-to-end streaming tests, including the failures.
- `docs/rust-stress-test/` - scenario list, signal definitions, tracker, provider
  failure policy, and how billing follows delivery rather than intent.
- `contracts/` - one contract package consumed by three runtimes: TypeScript at
  the edge, React in the browser, Rust on the media server. Contract version
  0.2.0.
- `infra/` - GPU transcode base images, task definitions and monitoring, all in
  OpenTofu.
- The `BRIVVA_*` environment flags across the Rust service: audio ducking,
  limiter, FIFO write budget, lag ceilings. These are the knobs that real
  streaming needs.

## Running it

It does not run out of the box. The stack expects AWS credentials, a Cloudflare
account, an Infisical workspace for secrets, and provider keys for the voice and
speech services. None of those are in this repository, and the original
environments are gone.

`docker-compose.yml` and `deploy.sh` show how the pieces were wired together.

## License

MIT. See `LICENSE`.
