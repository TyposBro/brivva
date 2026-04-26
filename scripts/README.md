# Scripts

Versioned shell tooling. Each script is idempotent and safe to rerun.

## Dev loop

### `test-all.sh` — every suite in parallel

Runs `cargo test` (server-rs), `bun run test` (workers), `bun run test` (frontend)
concurrently. Per-suite output streams to `.test-logs/<name>.log`.

```bash
./scripts/test-all.sh            # parallel (default)
./scripts/test-all.sh --serial   # sequential — readable interleave
```

Exit code = number of failing suites (0 = all green). Logs survive across
runs so you can `tail -n 80 .test-logs/server-rs.log` after a red.

### `dev-stack.sh` — boot → smoke → teardown

Brings up `server-rs` + `workers` locally, polls `/health`, runs
`smoke-test.sh local` against them, then shuts everything down.

```bash
./scripts/dev-stack.sh           # boot + smoke + stop
./scripts/dev-stack.sh --keep    # leave services running after smoke
```

Traps `EXIT / INT / TERM` so Ctrl-C always kills the children and frees
the ports. Logs land in `.dev-logs/{server-rs,workers}.log`.

Env knobs: `SERVER_PORT` (3000), `WORKERS_PORT` (8787), `BOOT_TIMEOUT` (60s).

Exit codes: `0` smoke passed · `1` smoke failed · `2` services failed to boot.

### `local-prod-parity.sh` — Fargate-shaped local smoke

Builds the same amd64 container shape used by Fargate: source-built
`ffmpeg-base` with librtmp/drawtext, then `server-rs/Dockerfile` with that
base image. It starts the docker-compose media smoke stack, verifies the
server container is `x86_64` and has `enable-librtmp` + `drawtext`, then runs
the RTMP media smoke.

```bash
./scripts/local-prod-parity.sh              # build + smoke + stop
./scripts/local-prod-parity.sh --skip-build # reuse local images
./scripts/local-prod-parity.sh --keep       # leave stack running
```

This gives FFmpeg/container parity with prod. External services are still
stubs unless you run the separate real-stack Playwright flow.

## Ops

### `smoke-test.sh` — running-stack sanity check

Post-rollback gate described in [`docs/runbook.md`](../docs/runbook.md).
Does NOT boot anything; hits `/health`, workers unauth check, OpenAPI doc,
and (on prod) ECS running-image sanity.

```bash
./scripts/smoke-test.sh prod     # against milliytechnology.* + Fargate
./scripts/smoke-test.sh local    # requires services already up
SERVER_URL=... ./scripts/smoke-test.sh   # custom URL
```

### `rollback.sh` — ECS task-def revert

See [`docs/runbook.md`](../docs/runbook.md) §Rollback. Short version:

```bash
./scripts/rollback.sh --list     # recent revisions with SHA-pinned images
./scripts/rollback.sh            # roll to the revision before current
./scripts/rollback.sh 12         # roll to brivva:12
```

### `../deploy.sh` — local ECR/ECS deploy

Builds and pushes the amd64 `server-rs` image locally, then registers and
deploys a new ECS task definition. It reuses the pinned prebuilt
`ffmpeg-base`, `server-build-base`, and `server-runtime-base` images by
default so normal deploys do not rebuild FFmpeg, reinstall Rust tooling, or
reinstall runtime fonts/libraries. The server image build also writes a
registry-backed BuildKit cache to `server-rs:buildcache` so cargo-chef
dependency layers survive GitHub cache misses.

```bash
./deploy.sh                       # build server-rs, deploy ECS
./deploy.sh --build-ffmpeg-base    # rebuild ffmpeg-base first, then server-rs
./deploy.sh --build-server-bases   # rebuild server build/runtime bases first
./deploy.sh --skip-build           # deploy current server-rs:latest
```

### `install-hooks.sh` — point git at versioned hooks

Sets `core.hooksPath = scripts/git-hooks`. Run once per clone. See
[`git-hooks/README.md`](git-hooks/README.md) for what each hook enforces.

## `.gitignore` expectations

These directories are created by scripts above. Add to `.gitignore` if not
already present:

```
.test-logs/
.dev-logs/
```
