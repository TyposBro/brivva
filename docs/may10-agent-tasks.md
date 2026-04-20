# May 10 Launch — Agent Task Prompts

Self-contained prompts for spawning agents on remaining P0 / P1 / P2 work
from `vision.md`. Each prompt briefs a cold agent with enough context to
act without reading this document.

**Generated:** 2026-04-20
**Source:** `vision.md` §"Round 2 Remaining" + §"Testing Debt" + §"Priority Stack"

Tasks map to `vision.md` table rows by number. See priority table there
for full context.

---

## How to Use

- Copy the prompt in a fenced block, paste into a fresh agent session.
- Prompts assume the agent has access to the repo root.
- Three-tier test mandate (claude.md §0) applies to every code-touching
  task: unit + integration + e2e.
- §0.5 Realism Rules apply: real fixtures, lifecycle matrices,
  multi-step chains, silent-path logs.
- §5.1 file-size hard limit (800L) applies.

Human-only tasks (voice QA with MJ, endurance drive, dress rehearsals,
launch day) are listed at the bottom without prompts.

---

## P0 — Round 2 Response-Shape Fixes

These block the quote / summary UX end-to-end. Ship before Week 2
endurance.

### Task 1 — Workers `/quote` per-lang breakdown

```
Brivva Tech, May 10 launch. Frontend pre-stream modal needs per-language cost itemization but Workers endpoint returns only aggregate.

Goal: Extend POST /api/sessions/:id/quote response with a `breakdown` array per target language.

Files:
- workers/src/orchestration/billing.ts (or wherever quote handler lives — grep for `/api/sessions/:id/quote`)
- contracts/ (Zod schema for QuoteResponse)
- frontend/src/features/broadcast/data/quote-api.ts (already expects `breakdown` — confirm shape)
- workers/tests/ — add case for 2-lang + 3-lang session

Current response: { output_minutes, estimated_cost_usd, rate_usd }
New response: { output_minutes, estimated_cost_usd, rate_usd, breakdown: [{ lang, output_minutes, cost_usd }] }

Rate comes from GET /api/billing/rate (flat $1.50/min). Passthrough langs (wire code "pass" or source==target) must be omitted from breakdown — they don't produce billable output.

Acceptance: fixture test with 3 target langs (ko→zh, ko→ja, ko→pass) returns 2 breakdown rows (zh, ja) with expected cost math. Frontend pre-stream modal renders without undefined errors.

Three-tier test mandate per claude.md §0: unit (handler logic), integration (hit endpoint with seeded session), e2e (pre-stream modal renders breakdown).
```

### Task 2 — Workers `/summary` shape + B2B variant

```
Brivva Tech, May 10 launch. Frontend post-stream summary modal keys off `total_minutes` / `total_cost_usd`; current Workers response has `source_minutes` / `output_minutes_by_lang`. B2B "Billed to" line never wired.

Goal: Reshape GET /api/sessions/:id/summary to match frontend expectations + add B2B variant.

Files:
- workers/src/orchestration/billing.ts (or handler for /summary)
- contracts/ (SummaryResponse schema)
- frontend/src/features/broadcast/presentation/summary-modal.tsx
- workers/tests/
- frontend/tests/ + frontend/e2e/

Spec from vision.md §Pricing UX:
  self-serve: { total_minutes, total_cost_usd, source_minutes, output_by_lang: {lang: minutes}, rate_usd }
  b2b:        { total_minutes, source_minutes, output_by_lang, billed_to: "studio@example.com" } — NO cost_usd line

Check users.billing_tier on session owner. If 'b2b', populate billed_to from users.bills_to and omit total_cost_usd.

Acceptance: modal shows duration, per-lang output minutes, and either "$189.00" or "Billed to Simon" based on tier. Three-tier tests.
```

### Task 4 — server-rs dispatch swap to active_voice_id

```
Brivva Tech. One-voice-per-user invariant (§Product Decision 3) enforced in Workers via users.active_voice_id pointer, but server-rs still reads session.voice_id on dispatch. Re-record replaces Workers pointer but old session.voice_id keeps dispatching stale clone.

Goal: server-rs session dispatch reads user.active_voice_id, not session.voice_id.

Files:
- contracts/ — regen after adding active_voice_id to User schema
- server-rs/src/features/broadcast/ — grep `session.voice_id` / `voice_id` usages
- server-rs/src/features/broadcast/data/pipeline/tts.rs (or wherever ElevenLabs voice_id is resolved)
- server-rs/tests/tts_cross_lang_clone.rs — add test proving re-record updates active dispatch

Migration 0008 already added users.active_voice_id. Session still carries voice_id (schema flexibility for future per-session override) but dispatch resolver ignores it for now.

Acceptance: integration test — create user with voice A, start session (session.voice_id=A), upsert new voice B via POST /api/voices, assert next TTS call uses voice B without restarting session. Three-tier.
```

---

## P1 — Quick Wins

### Task 3 — SEA defaults in stream-defaults.ts

```
Brivva Tech. stream-defaults.ts covers ko→zh/ja/en/ko + *→pass + _default. SEA expansion (Thai, Vietnamese, Indonesian) in scope per vision §Unit Economics but defaults missing — first SEA stream falls through to _default with wrong delay.

Goal: Add three rows to frontend/src/core/config/stream-defaults.ts:
  "ko→th": { delay_ms: 2500, host_gain: 0.2 }
  "ko→vi": { delay_ms: 2500, host_gain: 0.2 }
  "ko→id": { delay_ms: 2500, host_gain: 0.2 }

Acceptance: unit test proves lookup returns 2500/0.2 for ko→th, not _default fallback.
```

---

## P0 — Prod Hardening (ops, Aziz in loop)

### Task 6-8 — D1 migrations + Google OAuth prod

```
Brivva Tech, May 10 launch. Prod auth currently broken because D1 migrations 0005-0008 never applied to prod, Google OAuth redirect URIs not registered, GOOGLE_CLIENT_ID/SECRET not in Workers prod env.

Goal: Wire prod auth so any user can sign in.

Steps (coordinate with Aziz — some require his console access):
1. Inventory: `ls workers/drizzle/migrations/` — confirm 0005, 0006, 0007, 0008 exist and contain expected schemas (billing_tier, bills_to, active_voice_id, onboarding_completed_at).
2. Dry-run migrations against prod D1: `bun run --cwd workers migrate:prod --dry-run` (or wrangler d1 migrations apply --preview). Post output.
3. If clean, apply: `bun run --cwd workers migrate:prod`. Capture before/after schema diff.
4. List required Google Console redirect URIs from workers/src/auth/google.ts. Produce copy-paste list for Aziz.
5. Produce wrangler commands Aziz runs himself:
     wrangler secret put GOOGLE_CLIENT_ID --env prod
     wrangler secret put GOOGLE_CLIENT_SECRET --env prod
6. Post-apply smoke: hit prod /auth/google with a test Gmail, confirm /api/user/me returns a row.

DO NOT commit secrets. DO NOT run wrangler secret put yourself — surface the commands for Aziz.
```

---

## P1 — Testing Debt Follow-Ups

In-flight agents have scaffolded fixtures + some silent-path logs.
These close remaining gaps.

### Task 16 — Real fixture captures for Soniox + YouTube

```
Brivva Tech. P1a agent scaffolded 14 fixtures across 5 vendors. Grip tagged HAND_CRAFTED_PENDING_REAL_CAPTURE. Soniox + YouTube captures incomplete. Stripe deferred until pricing lands.

Goal: Replace hand-crafted fixtures with real API captures for Soniox + YouTube.

Steps:
1. Soniox: run one real STT session against live Soniox WS with ko source. Capture happy-path transcript + one error frame (send malformed audio to trigger error). Save to server-rs/tests/fixtures/soniox/ as real_*.json. Replace current hand-crafted if present. Update README status table.
2. YouTube: call real liveBroadcasts.insert + liveStreams.insert with a test Google account. Capture full response JSON including cdn.ingestionInfo. Save to workers/tests/fixtures/youtube/. Replace hand-crafted.
3. Add fixture-roundtrip test per vendor: load real_*.json → pass to existing deserializer → assert Ok + field values match capture.
4. Leave Stripe + Grip tagged pending (tracked separately).

Do NOT invent shapes. If Soniox response field is missing in real capture, delete it from the type — don't add it defensively.

Acceptance: both README status tables show CAPTURED for Soniox + YouTube. All fixture-roundtrip tests pass. claude.md §0.5.1 rule satisfied for 3/5 vendors.
```

### Task 17 — §0.5.4 log-audit verification

Status says "wired" but §0.5.4 demands post-merge verification. This
task closes it.

```
Brivva Tech. P1b agent added tracing::warn! calls to silent paths. claude.md §0.5.4 requires POST-MERGE audit: run a 30-min session, grep every silent branch, confirm ≥1 log line each.

Goal: Run the verification the rule actually demands.

Steps:
1. Grep server-rs for every `continue;`, `if let Ok(_)`, fallback branch in:
   - src/features/broadcast/data/pipeline/stt.rs
   - src/features/broadcast/data/pipeline/stt_response.rs
   - src/features/broadcast/data/pipeline/translate.rs
   - src/features/broadcast/data/pipeline/tts.rs
   - workers/src/**/*.ts fallback paths
2. Produce a checklist — one row per silent branch, with file:line.
3. Run a real 30-min session (local or staging — Aziz's call). Capture full stdout/stderr to log file.
4. For each checklist row, grep the log file for expected warn message. Mark PASS or MISSING.
5. Any MISSING rows → add the log call + commit + re-run.

Acceptance: 100% PASS in checklist. Commit the checklist as docs/post-merge-log-audit-2026-04-xx.md for reference.

Current status "wired but not verified" is not §0.5.4 compliant. This task is what closes it.
```

---

## P2 — Testing Debt (Remaining)

### Task 19 — Multi-step integration chain §0.5.3

```
Brivva Tech. Current integration tests chain at most 2 steps. Real bugs happen on 3+ step sequences (the April triad: session end → metrics loop tick → 404 → self-cancel).

Goal: Add integration test chaining POST → PATCH mid-flight → background-loop tick → DELETE → assert cleanup.

File: workers/tests/api.test.ts or new workers/tests/session-lifecycle-integration.test.ts

Scenario:
1. POST /api/sessions with 1 target lang
2. Verify streams table has 1 row
3. PATCH /api/sessions/:id — add second destination language mid-session
4. Verify streams has 2 rows, both pending
5. Manually tick the metrics reporter loop (simulate the interval)
6. Verify metrics reporter sees both streams
7. DELETE /api/sessions/:id (soft-end)
8. Tick metrics loop again
9. Verify reporter self-cancels for ended session (per MetricsReportError::NotFound self-cancel path with MAX_CONSECUTIVE_NOT_FOUND=2 — may need 2 ticks)
10. Verify final session row: status='ended', streams rows: status='ended'

Also add concurrent-mutation test: two simultaneous PATCH calls adding same lang — assert one wins, no duplicate streams row.

Acceptance: both tests pass. claude.md §0.5.3 satisfied. Covers the mutation-mid-flight class the April triad slipped through.
```

### Task 20 — E2E voice-clone cross-lang full chain §0.2

```
Brivva Tech. Voice-clone invariant (clone in lang A → stream to lang B sends language_code=A + model_id=eleven_flash_v2_5) split across 3 files, each covers a slice. No single e2e proves the full chain from UI to vendor wire payload.

Goal: One Playwright e2e that proves the full invariant via network intercept.

File: frontend/e2e/voice-clone-cross-lang-full-chain.e2e.ts

Scenario:
1. Sign in via mocked Google OAuth (test fixture)
2. Navigate /onboarding
3. Step 1: connect one platform (mock Grip paste)
4. Step 2: record 35s audio sample, submit with source_lang=ko (Korean script shown)
5. Step 3: pick default target lang=ja
6. Reach /dashboard
7. Create session: source=ko, target=[en]
8. Start stream
9. Intercept outgoing ElevenLabs TTS POST request
10. Assert request body contains:
    - language_code = "ko" (enrollment lang, not session source)
    - model_id = "eleven_flash_v2_5"
    - voice_id = pointer returned by POST /api/voices upsert
11. Also assert wire payload includes active_voice_id from user row, not a stale session.voice_id

Acceptance: e2e green. Catches the frontend-corruption class where UI sends wrong source_lang on clone POST — current invariant is server-side but UI can still drift.
```

### Task 15 — Grip Seller API evaluation

Blocked on vendor reply. Not May 10 critical path.

```
Brivva Tech. Grip has official Seller API (AccessKey+SecretKey) discovered 2026-04-19. Current integration uses paste-cred flow + reverse-engineered workarounds in docs/grip-integration-notes.md. Aziz emailed seller_support@gripcorp.co for API docs — reply pending.

Goal: When docs arrive, evaluate coverage and prepare migration behind BRIVVA_GRIP_USE_LEGACY kill-switch. NOT on May 10 critical path.

Steps (wait for vendor reply):
1. Parse Seller API spec. List every endpoint.
2. Map against current reverse-engineered operations in grip-integration-notes.md: provisionBroadcast, fetchStreamKey, startBroadcast, endBroadcast, health polling.
3. Coverage matrix: [operation] [current method] [Seller API equivalent] [gap].
4. If ≥80% covered, draft workers/src/integrations/grip/seller-api.ts behind kill-switch. Paste-cred path stays default until endurance validates.
5. Replace HAND_CRAFTED_PENDING_REAL_CAPTURE Grip fixtures with real captures from Seller API sandbox.
6. Add kill-switch integration test: BRIVVA_GRIP_USE_LEGACY=1 routes to old path, =0 routes to new.

Acceptance: coverage matrix posted. Kill-switch wired. Paste-cred untouched. No behavior change until explicit flag flip.

This is pure plumbing, not a May 10 blocker. Do after launch-day.
```

### Task 21 — CI pre-merge gate §0.6

Phase 2 blocker (self-serve opens after May 10). Not a May 10 blocker.

```
Brivva Tech. claude.md §0.6 defines 8-step pre-merge gate (tests, typecheck, contract drift, layer audit, three-tier coverage check, §5.1 file size check, §0.5.1 fixture presence, §0.5.4 log audit checklist). Currently not CI-enforced — discipline only. Any PR today can skip §0.5.1-4 and land.

Goal: Wire gate into GitHub Actions. Phase 2 blocker (self-serve onboarding opens after May 10); not a May 10 blocker.

Files to create:
- .github/workflows/pre-merge-gate.yml

Checks (each a separate job, all required for merge):
1. workspace-build — cargo build + bun install + tsc across workers, frontend, contracts
2. three-tier-tests — cargo test + bun test --cwd workers + bun test --cwd frontend + playwright
3. contract-drift — run contract regen, assert no diff
4. layer-audit — run existing layer-audit script, assert 0 violations
5. file-size — new check: any file >800L fails, any non-test file >500L warns
6. fixture-presence — check workers/tests/fixtures/**/*.json and server-rs/tests/fixtures/**/*.json non-empty for each vendor in a known list
7. log-audit — grep every `continue;` in pipeline code, assert preceding or following tracing::warn! within 3 lines
8. three-tier-coverage — for each new src file added in the diff, assert tests exist in unit + integration + e2e locations (configurable exemption list)

Branch protection: require all 8 before merge to main.

Acceptance: a toy PR that removes a warn! line fails check 7. A toy PR that adds a file >800L fails check 5. A toy PR with no tests for a new feature fails check 8.
```

### Task 23 — Remove demo-era cruft

```
Brivva Tech. vision.md §"What's Intentionally Not In The Product" says Brivva serves broadcasters only, not audiences. Demo-era artifacts still in code: /watch/:id route, "Quick Session" home-page button, "Audience Resume Session" input. Risk: reviewer sees them, thinks they're product features.

Goal: Delete audience-facing UI surface cleanly.

Files (grep for each):
- frontend/src/routes/ — remove /watch/:id
- frontend/src/features/home/ or landing — remove "Quick Session" CTA
- frontend/src/features/session/ — remove "Audience Resume Session" input
- any backend endpoints servicing /watch/:id or audience join — remove from workers + server-rs
- e2e tests that cover these — delete

Intended UI surface per vision.md:
  /, /auth/*, /onboarding, /dashboard, /session/:id/setup, /session/:id/live, /session/:id, /privacy, /terms

Acceptance: grep "Quick Session" / "Audience Resume" / "/watch/" returns zero hits across frontend + backend + e2e. Type checks pass. All existing broadcaster-facing flows unchanged.

Do NOT add anything. Pure deletion.
```

---

## Human-Only Tasks (no agent prompt)

| # | Task | Who | When |
|---|---|---|---|
| 5 | TikTok e2e integration | Aziz | in progress |
| 9 | Rollback + kill-switch rehearsal on prod (<5 min) | Aziz | Week 1 |
| 10 | Voice-clone accent bug verify with MJ (native Korean) | Aziz + MJ | Week 1 |
| 11 | 40-min Korean→Chinese(Grip)+Japanese(TikTok) dry run | Aziz + MJ | Week 2, May 1 |
| 12 | 2-session concurrent stress test | Aziz | Week 2 |
| 13 | CloudWatch alarm → phone wake path | Aziz | Week 2 |
| 14 | Lock best-sounding default Chinese voice IDs | Aziz | Week 2 |
| 22 | UX polish from dry-run findings | Aziz | Week 3 |
| 24 | Dress rehearsal x2 (happy + chaos/rollback) | Aziz | May 4-9 |
| 25 | Decide session timeout policy | Aziz + Simon | before rehearsal |
| 26 | Decide billing round-up rule | Aziz + Simon | before rehearsal |
| 27 | Decide recording retention window | Aziz + Simon | before rehearsal |
| 28 | Launch day smoke test + monitoring | Aziz | May 10 AM |

---

## Parallel Execution Safety

**Safe to run concurrently:** 1, 2, 3, 4, 15, 19, 20, 21, 23 — disjoint files.

**Serialize / coordinate:**
- **16 + 17** — both touch pipeline files (`stt.rs`, `stt_response.rs`,
  `tts.rs`). Run 17 after 16 lands, or hand each a different subset.
- **6-8** — requires Aziz in loop for secrets + console access.

---

## Critical Chain for May 10

```
10 (accent fix verify)
   → 6/7/8 (prod auth wired)
      → 11 (40-min endurance works)
         → 24 (dress rehearsal boring)
            → 28 (launch day)
```

Tasks 1, 2, 4 block quote/summary UX end-to-end. Ship before 11.

Everything else (15, 21, 23) is Phase 2 or polish — slip if needed.
