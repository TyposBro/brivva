# Testing Coverage

Per-package coverage baselines, critical-path policy, and how to run the
reports locally. Coverage is a floor, not a target — it's a signal that
if it drops we're eroding regression safety.

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
