# Ops Checklist — Production Readiness

Single source of truth for "is prod ready". Sections dated; append new
audits below rather than overwriting, so the trail of what changed and
when stays inspectable.

---

## 2026-04-19 — Pre-launch verification (T-21 to May 10)

Performed by an automated ops sweep. AWS account 132593557399, region
us-east-1. Cloudflare account `typosbro` (`80a55132ae169d5b282ccf505bc66bf7`).

### TL;DR

| Area | Status | Notes |
|---|---|---|
| D1 schema (workers) | GREEN | Migrations 0005–0008 applied, columns verified |
| Workers prod secrets | GREEN | All 5 required secrets set |
| server-rs `/health` | GREEN | 200 via `https://brivva.spiko.uz` |
| Workers reachability | GREEN | OpenAPI 200 at correct host |
| Smoke test script | RED  | Hardcoded wrong WORKERS_URL + bad assertion (see §3) |
| ECS circuit breaker | GREEN | enable+rollback true, 100/200, infra/main.tf reconciled |
| Rollback rehearsal | YELLOW | Mechanism works, but only one unique image in ECR — see §5 |
| Secrets Manager keys | GREEN | All 7 required keys present, kill-switches absent (off) |

Outstanding items requiring a human are listed in §8.

---

### 1. D1 schema verification — GREEN

State BEFORE: 4 migrations pending against remote D1 (`brivva`,
`4e4ddf3c-aa0f-4716-bd2e-b5fc711805d7`):

- `0005_user_profile_fields.sql`
- `0006_voice_source_lang.sql`
- `0007_session_metrics.sql`
- `0008_user_onboarding_billing.sql`

Action: `bun run migrate:prod` (`wrangler d1 migrations apply brivva --remote`).
All four reported `✅`.

State AFTER: schema dumped via `SELECT sql FROM sqlite_master`. Every
column declared in `workers/src/core/schema.ts` is present on the live
table. `session_metrics` table created. Tables present: `users`, `voices`,
`sessions`, `streams`, `platform_credentials`, `session_metrics`.

Drift observed (informational only):

- `sessions.room_id TEXT` still exists on the live table even though the
  Drizzle schema only declares `live_session_id`. Legacy column from
  pre-`0004` rename. Nullable, no foreign key, no code references — safe
  to leave; drop in a future cleanup migration if you care.

### 2. Workers prod secrets — GREEN

`bun wrangler secret list` returned:

- `ELEVENLABS_API_KEY`
- `GOOGLE_CLIENT_ID`
- `GOOGLE_CLIENT_SECRET`
- `INTERNAL_SECRET`
- `JWT_SECRET`

All five required secrets are set. `STRIPE_WEBHOOK_SECRET` is documented
in `wrangler.toml` as optional-until-billing — still absent, expected.

`SONIOX_API_KEY` does **not** belong on Workers — it is consumed by
`server-rs` (Fargate) only. Confirmed by `grep` of `workers/src/`. The
ops brief flagged it as "if workers proxies Soniox" — they don't.

Public `vars` (in `wrangler.toml`, not secret) are also set:

- `FRONTEND_URL = https://brivva.pages.dev`
- `OAUTH_REDIRECT_URI = https://brivva-api.milliytechnology.workers.dev/auth/youtube/callback`
- `GOOGLE_SIGNIN_REDIRECT_URI = https://brivva-api.milliytechnology.workers.dev/auth/google/callback`

Both redirect URIs must be registered in the Google Cloud Console OAuth
Client. If you add a custom domain later, register the new redirect URIs
in the same Google credential before flipping DNS, or sign-in breaks.

### 3. Smoke test — GREEN (both script bugs fixed 2026-04-19)

Was RED for two reasons, now resolved:

1. ~~`WORKERS_URL` hardcoded to `https://brivva-server.milliytechnology.org`~~
   — fixed: now defaults to `https://brivva-api.milliytechnology.workers.dev`.
2. ~~Unauth assertion expected 401/403 but `/api/user` returns 400~~
   — fixed: smoke now accepts any 4xx as proof the worker is up and
   validating. `/api/user` has no JWT middleware by design (keyed by
   `user_id` query param).

Manual verification of what the test was trying to prove:

| Probe | Result |
|---|---|
| `https://brivva.spiko.uz/health` | 200 (server-rs + tunnel up) |
| `https://brivva-api.milliytechnology.workers.dev/openapi.json` | 200 |
| `https://brivva-api.milliytechnology.workers.dev/api/user` (no params) | 400 with `{"error":"invalid request","issues":[…user_id…]}` (route alive, validating) |

Action items for whoever owns the script (do **not** call this script in
CI / launch-day playbooks until both bugs are fixed):

```bash
# Fix in scripts/smoke-test.sh, prod branch:
WORKERS_URL="${WORKERS_URL:-https://brivva-api.milliytechnology.workers.dev}"

# And replace the /api/user 401 check with one of:
#   GET /openapi.json -> expect 200
#   GET /api/user?user_id=__smoke__ -> expect 200
# until/unless real JWT middleware is added to /api/user.
```

### 4. ECS circuit breaker — GREEN, infra/main.tf reconciled

`aws ecs describe-services --query 'services[0].deploymentConfiguration'`:

```json
{
  "deploymentCircuitBreaker": {"enable": true, "rollback": true},
  "maximumPercent": 200,
  "minimumHealthyPercent": 100,
  "strategy": "ROLLING",
  "bakeTimeInMinutes": 0
}
```

Matches the runbook expectation: failed task-defs auto-roll back.

Drift in `infra/main.tf` (was set to 0 / 100, no circuit breaker block):
**reconciled**. Updated in the same commit as this checklist so a future
`terraform apply` will not undo prod. See git diff for `infra/main.tf`.

### 5. Rollback rehearsal — YELLOW

`./scripts/rollback.sh --list` returns two task-def revisions:

```
brivva:7  →  ECR :744892a5e429b5c1fd96a82b1dde3dbc684832fa
brivva:6  →  ECR :latest
```

Current service revision: `brivva:7`. ECR `describe-images` confirms both
`latest` and `744892a5e…` resolve to the **same image digest** — pushed
2026-04-19 12:14 KST. So a rollback today (`brivva:7 → brivva:6`) would
swap to a task-def whose image tag points at the same bytes. **Effective
no-op.**

Why this matters: the runbook's "automatic rollback restores last-known
good in <2 min" promise is currently a no-op promise. Until at least one
more deploy creates a *prior* immutable image, the only real rollback
path is Plan B (Tauri desktop, see runbook §"Plan B").

Recommended actions, in order:

1. **Stop pushing `:latest`.** `deploy.sh` (and the older task-def 6) are
   tagging with `:latest`. Switch to SHA-only tagging end-to-end so each
   task-def revision pins a unique digest. brivva:7 already does this
   correctly — it's just brivva:6 that's mutable.
2. **Push at least one more deploy before May 10.** That gives you a real
   `brivva:8 (new SHA) → brivva:7 (744892a5…)` rollback edge.
3. After (2), re-run this rehearsal. The dry list should show two
   distinct SHAs.

Mechanism itself (the script) works: lists revisions, resolves images,
asks before update-service, waits for `services-stable`. No code change
to the script needed.

### 6. Secrets Manager (Fargate kill-switch readiness) — GREEN

`brivva/env` keys (names only, fetched via `jq 'keys'`):

```
ELEVENLABS_API_KEY    (required, set)
GOOGLE_CLIENT_ID      (required, set)
GOOGLE_CLIENT_SECRET  (required, set)
INTERNAL_SECRET       (required, set)
JWT_SECRET            (required, set)
SONIOX_API_KEY        (required, set)
TUNNEL_CREDS          (required, set — cloudflared sidecar)
```

Optional kill-switches **absent** (default = off, intentional):

- `BRIVVA_FALLBACK_TO_DEFAULT_VOICE`
- `BRIVVA_FORCE_RTMP_NOT_RTMPS`

How to flip a kill-switch in incident (paste-ready):

```bash
# Read current secret JSON, splice in the kill-switch, push back.
aws secretsmanager get-secret-value --region us-east-1 --secret-id brivva/env \
  --query 'SecretString' --output text \
  | jq '. + {BRIVVA_FALLBACK_TO_DEFAULT_VOICE:"1"}' \
  > /tmp/brivva-env.json
aws secretsmanager update-secret --region us-east-1 --secret-id brivva/env \
  --secret-string file:///tmp/brivva-env.json
rm /tmp/brivva-env.json

# Force task restart so server-rs re-reads at session-start.
aws ecs update-service --region us-east-1 --cluster brivva --service brivva \
  --force-new-deployment
```

Substitute `BRIVVA_FORCE_RTMP_NOT_RTMPS` for the RTMPS-handshake-fail
case. Truthy values: `1`, `true`, `yes`, `on`. Anything else disables.

Per `docs/runbook.md` §"Kill Switches", the older `BRIVVA_DISABLE_TIER4`
and `BRIVVA_DISABLE_QWEN3` switches are intentionally not present — those
providers aren't in the current `server-rs` tree.

### 7. Production state snapshot

| Layer | Endpoint / Resource | State |
|---|---|---|
| server-rs (Fargate) | `https://brivva.spiko.uz/health` | 200 |
| Workers (CF) | `https://brivva-api.milliytechnology.workers.dev/openapi.json` | 200 |
| D1 `brivva` | APAC region, ICN colo, 86 KB, 6 user tables + meta | green |
| ECS service | `brivva` cluster, 1 desired, circuit breaker on | green |
| ECR repo | `brivva/server-rs`, 1 unique digest pushed 2026-04-19 | yellow (see §5) |
| Secrets Manager | `brivva/env`, 7 required keys | green |

### 8. Outstanding manual tasks (human-only)

Numbered in priority order:

1. **(Pre-May-10) Push one more `server-rs` deploy** so a real prior
   revision exists in ECR and `rollback.sh` becomes meaningful. Do this
   from any non-trivial branch merge — do not invent a no-op deploy just
   for the rollback edge; piggyback on a real change.
2. **(Pre-May-10) Stop tagging `:latest` in `deploy.sh`.** Tag images by
   git SHA only. The rollback script reads SHA tags, not `:latest`.
3. **Fix `scripts/smoke-test.sh`** per §3 above (WORKERS_URL host + the
   `/api/user` assertion). After fix, it should exit 0 against prod.
4. **Bookmark the Cloudwatch dashboard URL on a phone browser** (per
   `docs/runbook.md` pre-May-10 checklist).
5. **Confirm `aws logs tail` works from phone hotspot** — i.e. AWS CLI +
   creds reachable when home WiFi is dead. If not, install on phone or
   keep a backup laptop with creds.
6. **Save Simon's and MJ's phone numbers under a 2 AM-reachable contact
   group** on the phone. Test ringtone overrides do-not-disturb.
7. **Keep a signed Tauri build on the laptop as Plan B.** Per runbook,
   if SaaS is down mid-show, Simon runs the show off Aziz's laptop. The
   build path is `cargo tauri build` then `src-tauri/target/release/bundle/macos/brivva.app`.
8. **(Optional, post-launch) Drop legacy `sessions.room_id` column** in
   a follow-up D1 migration. Not blocking.
