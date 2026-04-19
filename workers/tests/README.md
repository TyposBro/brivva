# workers/tests

Two kinds of tests live here. Keep them separate.

## `api.test.ts` — endpoint unit / behaviour tests

Covers **business logic**:
- status codes (200 / 400 / 401 / 404 / 409 / 500)
- side effects (DB writes, FK cascades, fetch-stubbed ElevenLabs / Google / Stripe calls)
- idempotency, validation branches, voice-sample limits, CORS, defaults/clamping
- happy + sad + edge paths per endpoint

Allowed to hand-pick response fields and assert against ad-hoc shapes. This is where the bulk of coverage lives (~99% lines).

## `contract-roundtrip.test.ts` — FE/Workers contract tests

Covers **one thing only**: "does the handler's response body actually parse with the FE's Zod schema?"

- imports the real `@brivva/contracts/http` schemas (no mocks, no copies)
- calls the real Hono app via miniflare
- runs `schema.safeParse(body)` and fails loud if `success=false`

No status codes, no business logic, no fetch-stub coverage. If you're writing a test that asserts anything other than "body matches contract", it belongs in `api.test.ts`.

### Why this file exists

On 2026-04-19 a `cost_usd` vs `estimated_cost_usd` drift landed in production. Both sides had ~99% line coverage. Workers tests asserted their own wrong key; FE tests asserted their own correct key against a handcrafted fixture. Neither side's suite crossed the boundary, so the drift was invisible until a user saw a raw Zod issues blob in the quote modal.

Round-trip tests cross the boundary. They're the cheapest insurance against "we renamed one field and the other side didn't notice".

### Scope rule

One happy + one drift-prone edge per endpoint. Resist the urge to turn this file into a second copy of `api.test.ts`. If it grows past 5–10 endpoints, first question: is `@brivva/contracts/http` still hand-written, or could we generate schemas directly from the OpenAPI spec and make this file unnecessary?

## Other files

- `setup.ts` — applies D1 migrations once per worker, wipes tables between tests
- `env.d.ts` — typed bindings for `cloudflare:test`
- `db.test.ts` — Drizzle-level CRUD tests (no HTTP)
- `auth.test.ts`, `elevenlabs-client.test.ts`, `google-oauth-client.test.ts`, `google-signin-client.test.ts`, `stripe-webhook.test.ts`, `wav-duration.test.ts` — per-module unit tests for shared/features code
