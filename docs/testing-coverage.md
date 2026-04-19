# Testing Coverage

Per-package coverage baselines, critical-path policy, and how to run the
reports locally. Coverage is a floor, not a target — it's a signal that
if it drops we're eroding regression safety.

## server-rs (`server-rs/`)

### How to run

```bash
cargo install cargo-llvm-cov --locked    # one-time
cargo llvm-cov --workspace --summary-only \
  --ignore-filename-regex 'main\.rs|ffmpeg/orphan\.rs'
```

HTML drill-down:

```bash
cargo llvm-cov --workspace --html \
  --output-dir target/coverage \
  --ignore-filename-regex 'main\.rs|ffmpeg/orphan\.rs'
open target/coverage/html/index.html
```

Missing lines per file:

```bash
cargo llvm-cov report --show-missing-lines \
  --ignore-filename-regex 'main\.rs|ffmpeg/orphan\.rs'
```

### Provider + excluded files

`cargo-llvm-cov` runs the Rust compiler with LLVM source-based
instrumentation and aggregates profraw files across the whole workspace.
Two files are excluded from the denominator because they contain no
logic testable without real FFmpeg or a full-boot process:

| File | Why excluded |
|---|---|
| `src/main.rs` | Axum `#[tokio::main]` + tracing init + bind. Covered by integration tests starting the app. |
| `src/features/broadcast/data/ffmpeg/orphan.rs` | Wraps `pgrep` + `/tmp/` scan; runs once at process startup, exercised by the e2e docker smoke. |

### Baseline + current

```
Baseline (pre-sweep, commit ce15a4a):  43.14% lines / 41.97% regions / 48.21% functions
Current:                                ≥ 91%    lines / ≥ 91%     regions / ≥ 94%    functions
```

### Critical paths — 100% branch

| File | Line | Why |
|---|---|---|
| `features/broadcast/data/auth.rs` | 100% | JWT verify; every failure mode covered (secret missing, wrong iss/aud, expired, signature mismatch, garbage). |
| `features/broadcast/data/ffmpeg/mixer.rs` | 100% | PCM gain + mix: clipping, odd-byte tail, zero-length, len mismatch. |
| `features/broadcast/data/state.rs` | 100% | Feature state init + shared Arc. |
| `features/broadcast/data/workers_api.rs` | 99.75% | All three HTTP methods × empty-url/network/non-2xx/malformed-json/success branches. |
| `features/broadcast/data/metrics.rs` | 99.4% | Reporter task success/404/stop-before-tick/log-only branches. |
| `features/broadcast/domain/metrics.rs` | 100% | Counters + snapshot + serialization. |
| `features/broadcast/data/pipeline/tts.rs::resolve_voice` | 100% | `tests/kill_switch.rs` + inline unit tests. |
| `features/broadcast/data/session_ws.rs::maybe_downgrade_rtmps` | 100% | `tests/kill_switch.rs`. |
| `orchestration/config.rs::env_flag` | 100% | Every truthy/falsy case inc. case-insensitive + non-literal. |
| `orchestration/router.rs` | 100% | `/`, `/health`, 404. |
| `features/broadcast/data/pipeline/soniox.rs` | 100% | Mode routing + config serialization. |

### Known gaps (intentional)

- `features/broadcast/data/ffmpeg/mod.rs::spawn_stream_inner` — real
  FFmpeg spawn path is not unit-tested. Exercised end-to-end by the
  docker-compose smoke in `tests/e2e/`. Pure helpers like
  `build_ffmpeg_args` and all `RtmpManager::push_*` / `detect_crashed` /
  `kill_idle_streams` branches ARE unit-tested using a short-lived
  `sh -c "exit 0"` as a stand-in child.
- `features/broadcast/data/ffmpeg/drain.rs` normal-flow loop body — the
  stop-flag exit + write-failure exit + pure helpers (`take_tick_sample`,
  `drain_aged_host_audio`, `drain_ready_frame`, `build_tick_output`,
  `wait_*_tick`) are unit-tested; the multi-minute tick sequence is
  exercised only by the e2e smoke.
- `features/broadcast/data/pipeline/stt.rs::run_soniox_session`
  reconnect-loop body after the first attempt — tokio time isn't paused
  in tests, so the 1–3s backoff × 5 attempts cost isn't worth the
  wall-clock. The session-gone branch is covered.

### Test conventions

- `should_<behavior>_when_<condition>` naming — CLAUDE.md §0.2.
- Co-located `#[cfg(test)] mod tests` when the fake is lightweight.
- Separate integration tests under `server-rs/tests/` when a full app
  boot + JWT-signed WS handshake is needed.
- **No dependency on the FFmpeg binary, Soniox, ElevenLabs, or external
  network.** HTTP/WS mocks bind ephemeral `127.0.0.1:0` via Axum routers
  or `tokio_tungstenite::accept_async`.
- For tokio tasks normally run on a long interval, expose a
  `pub(crate) *_with_interval` variant keyed on `Duration` — see
  `data::metrics::spawn_metrics_reporter_with_interval`.

### When coverage drops

If a PR drops server-rs overall line coverage below 90% or a
critical-path file below 100%, the reviewer should request either:

1. A test that exercises the new path, or
2. An explicit `// PRAGMATIC: <reason>` comment near the uncovered line
   and a note in the PR description.

Same policy as Workers.

## Workers (`workers/`)

### How to run

```bash
bun run --cwd workers test:coverage
```

Renders a `text` summary + writes `workers/coverage/index.html` for
drill-down. JSON summary available with:

```bash
bun run --cwd workers test:coverage -- --coverage.reporter=json-summary
cat workers/coverage/coverage-summary.json
```

### Provider choice — istanbul, not v8

Workers tests run inside `@cloudflare/vitest-pool-workers` (miniflare →
workerd). The `v8` provider uses `node:inspector.Session`, which workerd
doesn't implement — calls fail with `ERR_METHOD_NOT_IMPLEMENTED` and no
coverage is produced. Istanbul instruments source at transform time, so
the instrumented modules run fine inside workerd.

Excluded from coverage:
- `src/orchestration/openapi.ts` — static OpenAPI schema document, no
  behavior to test. Covered by the api.test.ts spec assertions.
- `src/**/*.d.ts` — generated contract typings.

### Coverage floor

| Scope | Lines | Branches |
|---|---|---|
| Overall (`All files`) | ≥ 90% | ≥ 85% |
| Critical paths (see list below) | 100% | 100% |
| Non-critical features | ≥ 90% | ≥ 80% |

Current snapshot at commit time: **99.08% lines / 87% branches overall.**

### Critical paths (100% line + 100% branch)

These files guard money + user identity. Regressions here are silent and
expensive, so every branch has a test.

| File | What it guards |
|---|---|
| `features/auth/google-signin-client.ts` | Who the user is. Sub verification + profile upsert. |
| `features/billing/stripe-webhook.ts` | Payment events. HMAC sig verify, timestamp tolerance, constant-time compare. |
| `shared/audio/wav-duration.ts` | Voice sample gate (30s floor, 180s cap, malformed WAV rejection). |
| `shared/auth/jwt.ts` | Short-lived WS-handoff JWT. HS256 sign + verify, iss/aud/exp edge cases. |

### Best-effort (high coverage, not 100%-enforced)

| File | Line | Branch | Why not 100% |
|---|---|---|---|
| `shared/db/db.ts` | 100% | 90% | Defensive `if (!row) throw` after upsert — unreachable without mocking drizzle itself. |
| `orchestration/app.ts` | 99.7% | 81% | Hono `??` short-circuits + a single dead `invalid()` fallback branch reachable only if Zod emits a non-ZodError shape. |
| `core/schema.ts` | 73% | 100% | Drizzle `.references(() => ...)` lambdas fire lazily inside drizzle-kit, never from our query paths. |

### Outbound-fetch policy

**No real network in any test.** All outbound calls to ElevenLabs,
Google OAuth, userinfo, etc. are stubbed via `vi.stubGlobal("fetch", ...)`.
Two patterns in the test suite:

- `installFetchStub([{ match, method, reply }])` in `tests/api.test.ts`
  for multi-call flows (OAuth dance + subsequent API hit).
- Inline `vi.stubGlobal("fetch", vi.fn(async (url, init) => ...))` in
  the per-feature unit tests (e.g. `google-signin-client.test.ts`).

If a test ever surfaces an `unstubbed fetch: <url>` error, that's by
design — it prevents a silent hit on prod OAuth endpoints.

### Test layout

```
workers/tests/
├── setup.ts                        # D1 migrations + per-test truncate
├── api.test.ts                     # Hono route integration (miniflare D1)
├── db.test.ts                      # db.ts unit tests (real D1)
├── wav-duration.test.ts            # WAV probe + validator
├── stripe-webhook.test.ts          # HMAC verify unit tests
├── google-signin-client.test.ts    # OAuth sign-in client unit tests
├── google-oauth-client.test.ts     # YouTube OAuth client unit tests
└── elevenlabs-client.test.ts       # ElevenLabs clone/delete proxy
```

### When coverage drops

If a PR drops overall line coverage below 90% or a critical-path file
below 100%, the reviewer should request either:

1. A test that exercises the new path, or
2. An explicit `// PRAGMATIC: <reason>` comment near the uncovered line
   and a note in the PR description.

No silent drops.
