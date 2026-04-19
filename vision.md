# Brivva Tech — Product Vision & May 10 Roadmap

Last updated: 2026-04-19
Owner: Aziz (tech) + Simon (sales) + MJ (client acquisition)
First live show: `2026-05-10`

---

## One-Line Vision

**Shopify for live-commerce translation.** Creators speak once, broadcast
globally in their own cloned voice, pay by the minute.

We compete with Prism (live broadcasting) and Dubly (post-processed
dubbing) by combining both into one real-time product.

---

## Customer Tiers

| Tier | Who | Onboarding | Billing |
|---|---|---|---|
| **B2B** (launch target) | Korean live-commerce studios Simon + MJ already sell to (~60 contracts waiting) | Manual: Simon/MJ create accounts, hand off creds | Monthly invoice, billed to studio |
| **Self-serve** (post-launch) | Indie creators, smaller shops | Google sign-in → Stripe → stream | Stripe metered, per output minute |

Both use the same pipeline. Only onboarding + billing differ. B2B clients
are internally flagged by setting `users.billing_tier = 'b2b'` and
`users.bills_to = <invoice-email>`; everything else is identical.

---

## Unit Economics

- **Charge**: `$1-2` per OUTPUT minute (not per source minute, not per language)
- **Cost**: `~$0.10` per output minute (Soniox STT + ElevenLabs TTS + Fargate)
- **Margin**: ~90%
- **Output minutes** = source minutes × target language count
  (200 source min/mo per client × 3 target langs = 600 output min/mo)
- **Aziz share**: `30%` of profit (may push to `34%` at entity formation)

### Revenue projections (10 clients, 200 source min each, $1.50/output min)

| Languages | Output min/mo | Revenue | Profit | Aziz 30% |
|---|---|---|---|---|
| 1 (Chinese only) | 2,000 | $3,000 | $2,800 | $840 |
| 2 | 4,000 | $6,000 | $5,600 | $1,680 |
| 3 | 6,000 | $9,000 | $8,400 | $2,520 |
| 5 (+ SEA) | 10,000 | $15,000 | $14,000 | $4,200 |

SEA expansion (Thai / Vietnamese / Indonesian) is pure multiplier — same
source audio, more output minutes, negligible extra cost.

---

## User Flows

### Flow A — B2B Host (Gitae, first May 10 user)

```
1. Simon creates Gitae's account
   ├─ Google sign-in with Gitae's Gmail
   └─ /auth/google → users row upserted → JWT → /onboarding

2. /onboarding wizard (3 steps, required)
   ├─ Step 1: Connect ≥1 platform (YouTube OAuth / Grip paste / TikTok paste)
   ├─ Step 2: Record voice sample (30s min, 180s max)
   └─ Step 3: Pick default target language
   → users.onboarding_completed_at set → /dashboard

3. Dashboard
   ├─ Create session: pick source_lang=ko, target_langs=[zh, ja]
   ├─ For each target: pick destination platform
   │     zh → Grip          (defaults: delay=3000ms  gain=0.2)
   │     ja → TikTok        (defaults: delay=1500ms  gain=0.2)
   │     ko → YouTube       (passthrough: delay=0ms  gain=1.0)
   └─ Advanced toggle (collapsed) to tune delay+gain if needed

4. Pre-stream quote
   ├─ Slider: "How long do you expect to stream?" (10-180 min)
   ├─ Quote: expected_minutes × target_count × rate
   └─ "Go Live" button

5. Live show
   ├─ /session/:id/live — mic+cam, WS to Fargate
   ├─ Fargate: Soniox STT → translate → ElevenLabs TTS → FFmpeg → RTMP
   ├─ Multi-platform broadcast runs simultaneously
   └─ Top-right cost-so-far counter ticks every minute

6. End stream
   ├─ Post-stream confirmation modal
   ├─ Shows actual source_minutes, output_by_lang, cost
   └─ Tier 4 VOD option (not in scope for May 10)

7. End of month
   ├─ B2B: Simon pulls session_metrics for billing_tier='b2b', invoices
   └─ Self-serve (later): Stripe auto-charges card
```

### Flow B — Simon (operator)

For May 10: direct D1 queries via `wrangler d1 execute`. See
`docs/b2b-onboarding-notes.md` for the snippets (coming in Round 2).
Admin dashboard UI is **post-launch**.

### Flow C — 2AM Incident (Aziz)

Documented end-to-end in `docs/runbook.md`. Quick reference:

```bash
# Look
aws logs tail /ecs/brivva --follow
wrangler tail

# Kill-switches (no redeploy, just restart)
BRIVVA_FALLBACK_TO_DEFAULT_VOICE=1   # voice clone broken
BRIVVA_FORCE_RTMP_NOT_RTMPS=1        # TLS breaks with a platform

# Rollback
./scripts/rollback.sh --list
./scripts/rollback.sh <prev-rev>
./scripts/smoke-test.sh prod
```

ECS deployment circuit breaker auto-rollbacks failed deploys.

---

## Product Decisions (Locked Apr 19)

These shape every downstream spec. Document them in the code, not in
conversation memory that fades.

### 1. Onboarding → Dashboard

After Google sign-in, route based on `users.onboarding_completed_at`:
- Falsy → `/onboarding` (3-step wizard, required)
- Set → `/dashboard`

Wizard steps: connect ≥1 platform, record voice (30s–180s), pick default
target language. On completion, POST `/api/user/complete-onboarding`
sets the timestamp.

### 2. Per-Stream Config Hidden + Smart Defaults

Default lookup table lives in
`frontend/src/core/config/stream-defaults.ts`:

```typescript
export const STREAM_DEFAULTS = {
  "ko→zh": { delay_ms: 3000, host_gain: 0.2 },
  "ko→ja": { delay_ms: 1500, host_gain: 0.2 },
  "ko→en": { delay_ms: 2000, host_gain: 0.2 },
  "ko→th": { delay_ms: 2500, host_gain: 0.2 },
  "ko→vi": { delay_ms: 2500, host_gain: 0.2 },
  "ko→id": { delay_ms: 2500, host_gain: 0.2 },
  "ko→ko": { delay_ms: 0,    host_gain: 1.0 },
  "_default": { delay_ms: 2500, host_gain: 0.2 },
};
```

95% of users never open the Advanced toggle. Over time, analyze
`session_metrics` + user retry behavior to auto-update the defaults.
Not automated for May 10.

### 3. One Voice Clone Per User

Enforced via `users.active_voice_id` pointer. `POST /api/voices`
upserts — deletes old ElevenLabs voice + D1 row, creates new, updates
pointer. UI shows singular "Your voice" + "Re-record" button.

Schema stays multi-row on `voices` table for future flexibility
(per-session voice selection), but UX is always singular today.

### 4. B2B Onboarding = Manual SQL

Simplest wins. Same Google sign-in for everyone. Simon flags B2B
clients via direct D1 update after they sign up:

```sql
UPDATE users
SET billing_tier = 'b2b', bills_to = 'studio@example.com'
WHERE email = 'gitae@example.com';
```

Two new columns on `users`:
- `billing_tier TEXT DEFAULT 'self_serve'` (`self_serve | b2b`)
- `bills_to TEXT` (email for B2B invoice reference)

Admin UI for Simon is **post-launch**. Real `/admin` endpoint waits
until demand proves the need.

### 5. Pricing UX — Pre-Stream Quote + Post-Stream Confirm

Pre-stream modal on "Go Live" click:
```
┌─ Ready to broadcast ────────────────────────┐
│ Expected duration: [ 60 ]──●──── 180 min    │
│ Languages: Korean → Chinese, Japanese        │
│ Estimated: 120 output min × $1.50 = $180    │
│ [  Go Live  ]                [  Cancel  ]    │
└──────────────────────────────────────────────┘
```

Mid-stream: subtle top-right counter showing elapsed + cost-so-far.

Post-stream confirmation modal:
```
┌─ Stream ended ──────────────────────────────┐
│ Duration: 63 minutes                         │
│ Source audio:    63 min                      │
│ Chinese output:  63 min                      │
│ Japanese output: 63 min                      │
│ Total billable:  126 output min              │
│ Cost: $189.00                                │
│ [  OK  ]                                     │
└──────────────────────────────────────────────┘
```

B2B users see the same modal but with "Billed to Simon" note instead
of the raw cost line.

API endpoints:
- `GET /api/billing/rate` → `{ per_output_minute_usd: 1.50 }`
- `POST /api/sessions/:id/quote` → `{ output_minutes, cost_usd, rate_usd }`
- `GET /api/sessions/:id/summary` → live or final metrics rollup

---

## What's In Scope For May 10

### Already Shipped

| Layer | What |
|---|---|
| server-rs | Clean 4-layer architecture, WebSocket + FFmpeg pipeline, per-language video delay, audio mixing (host/TTS), source-lang passthrough, 2 kill-switches wired (`BRIVVA_FALLBACK_TO_DEFAULT_VOICE`, `BRIVVA_FORCE_RTMP_NOT_RTMPS`), RTMPS verified for Grip, idle-restart for Grip regional drops, SessionMetrics export task, kill-switch integration tests |
| workers | D1 schema (users, voices, sessions, streams, platform_credentials, session_metrics), Google OAuth for sign-in + YouTube OAuth for add-channel, Grip/TikTok credential paste endpoints, voice clone with source_lang language labels, WAV duration validation (30s min, 3min max), Stripe webhook signature verifier stub, session usage + billing summary endpoints |
| frontend | In-memory JWT auth, SignInGate route guard, voice recorder 30s–180s with progress bar, host-page split (/setup + /live), session-page observability strip (live minutes + cost estimate), dashboard with platform region grouping |
| infra | Terraform ECR + ECS + IAM + CloudWatch + Secrets Manager, GitHub OIDC deploy, SHA-pinned images, ECS deployment circuit breaker, cargo-zigbuild cross-compile (5min builds), `./scripts/rollback.sh`, `./scripts/smoke-test.sh` |
| CI/CD | Contracts drift check, server-rs + workers + frontend typecheck/test/build, layer audits (0 violations across all 3), pre-commit + pre-push + post-merge git hooks |
| docs | ARCHITECTURE.md, runbook.md, grip-integration-notes.md, terraform-state-migration-plan.md, this file |

### Round 2 (in progress as of Apr 19)

| Agent | Task |
|---|---|
| Frontend | `/onboarding` 3-step wizard |
| Frontend | `stream-defaults.ts` table + collapsed Advanced toggle on destination card |
| Frontend | Pre-stream quote modal (duration slider) |
| Frontend | Post-stream summary modal |
| Frontend | Singular "Your voice" + "Re-record" button |
| Workers | Migration `0008_user_onboarding_billing.sql` (active_voice_id, onboarding_completed_at, billing_tier, bills_to) |
| Workers | `POST /api/user/complete-onboarding` |
| Workers | `POST /api/voices` becomes upsert (deletes old clone, updates active_voice_id) |
| Workers | `GET /api/billing/rate` |
| Workers | `POST /api/sessions/:id/quote` |
| Workers | `GET /api/sessions/:id/summary` |
| Workers | `docs/b2b-onboarding-notes.md` with wrangler snippets |
| server-rs | (no new work) Regen contracts when Workers ships active_voice_id; swap session.voice_id → user.active_voice_id at dispatch |

### Round 3 Candidates (if endurance test surfaces bugs)

- Indian-accent regression if voice_settings + source_lang hint doesn't actually fix it — fallback is `eleven_multilingual_v2_5` model_id or strict enrollment-lang mismatch forcing default voice
- Chinese voice quality if default library voice sounds robotic — pick better voice IDs
- Concurrent session handling if ECS scaling is flaky at 2-3 parallel streams

---

## What's Intentionally Not In The Product

**Brivva does NOT serve end viewers.** End audiences watch on the
streaming platforms (YouTube, Grip, TikTok, etc.) where the RTMP output
lands. The Brivva frontend is operator-facing only — broadcasters +
Simon/MJ. No "Join Session", no "Audience Resume", no "Quick Session"
/ no-auth demo button on the landing page. These were demo-era artifacts
from before the pivot to RTMP-out SaaS.

If anyone tries to re-introduce audience-facing UI (another "Quick
Session" on the home page, a `/watch/:id` route, a public session list,
etc.), it's a drift from the product and should be rejected at review.

The Brivva UI surface is:
- `/` — public landing with a single CTA to Stream Dashboard (OAuth gate)
- `/auth/*` — OAuth handshake
- `/onboarding` — 3-step wizard for first-time broadcasters
- `/dashboard` — session + voice + platform management for broadcasters
- `/session/:id/setup` — per-session platform + lang config
- `/session/:id/live` — live broadcast view for the host
- `/session/:id` — post-stream summary for the host
- `/privacy`, `/terms` — legal

Everything else is out of scope.

## What's NOT In Scope For May 10

Explicitly deferred. Do not let these creep in.

- **Stripe real billing** — B2B doesn't need it; self-serve is post-launch
- **Tier 4 post-processed VOD** — no current customer demand
- **Admin dashboard for Simon** — direct D1 queries work; UI later
- **Multi-region Fargate** — us-east-1 only; performance is fine for Korean + SEA
- **Terraform S3 state cutover** — scheduled post-May-10 (infrastructure prepped, disabled pending drift resolution)
- **Naver OAuth / Apple Sign-In** — Google only until demand proves otherwise
- **Instagram integration** — Grip + TikTok + YouTube cover the primary market
- **Mobile apps** — web-only for launch
- **Per-language pricing** — flat `$1.50/min` across all languages for launch
- **Incorporation details** — Brivva Tech entity formation (34% equity push) is parallel, not blocking

---

## 21-Day Countdown To May 10

### Week 1 — Production Hardening + MJ Dry Run (Apr 20-26)

**Highest priority: verify voice-clone accent bug with MJ's Korean voice.**

MJ is always available and speaks native Korean. Use her for the accent-
bug verification test this week — no need to wait for Gitae's weekend
slot. If the fix works with MJ, repeat validation with Gitae on the
weekend for extra confidence.

```
1. Record 3-min Korean voice sample with MJ
2. Enroll through /onboarding → voice step (verify 30-180s flow end-to-end)
3. Broadcast Korean → Chinese AND Korean → English for 2 minutes each
4. Listen: does English come out Indian accent?
5. If yes → Round 3 server-rs fix options:
   - Upgrade to eleven_multilingual_v2_5 model
   - Strict enrollment-lang mismatch forcing default voice
   - Different voice_settings tune (stability/similarity_boost)
6. If no → great, schedule weekend validation with Gitae
```

**Also this week:**
- Apply D1 migrations 0005-0008 to prod (`bun run --cwd workers migrate:prod`)
- Register production Google OAuth redirect URIs in Google Console
- Put `GOOGLE_CLIENT_ID` + `GOOGLE_CLIENT_SECRET` in Workers prod env via `wrangler secret put`
- Rehearse rollback end-to-end (`rollback.sh --list → rollback → smoke-test → roll-forward`) — target under 5 min without notes
- Rehearse kill-switches on prod — flip each, verify behavior, flip back

### Week 2 — Real Platform Integration (Apr 27 - May 3)

`May 1` = planned endurance test per Simon.

MJ drives from the host side (Korean). Aziz drives from ops. Gitae joins
on the weekend if accent-bug validation needs a second native voice.

- First full dry run with shared Grip + TikTok creds (MJ speaks Korean)
- Stream Korean → Chinese (Grip) + Japanese (TikTok) for 40 minutes
- Document every bug, fix criticals same-day
- Lock in best-sounding default Chinese voices (pick from ElevenLabs library)
- Concurrent session test: 2 browsers, 2 hosts, simultaneous streams
- Monitoring/alerting check: does a CloudWatch alarm actually wake Aziz?

### Week 3 — Polish + Dress Rehearsal (May 4 - May 9)

- UX polish from dry run: loading states, error messages, "you're live" indicator
- Run full flow end-to-end TWICE:
  1. Happy path: sign up → stream 30min → end → check summary
  2. Chaos: start → kill-switch mid-stream → rollback → smoke → resume
- Second pass should be boring. If not, fix until boring.
- Emergency kit ready: Simon/MJ phone numbers in favorites, laptop charged with
  signed Tauri build as Plan B, CloudWatch dashboard on phone, kill-switch
  names memorized

### May 10 — Go Live

**Morning:**
- `./scripts/smoke-test.sh prod` → green
- `aws ecs describe-services` → task stable
- Confirm Simon + Gitae awake + ready

**During the show:**
- `aws logs tail /ecs/brivva --follow` in one terminal
- `wrangler tail` in another
- CloudWatch dashboard in another tab
- Phone off-silent
- Don't touch anything unless something breaks

**After:**
- Celebrate with Simon
- Review `session_metrics`, confirm billing numbers against the real stream
- Document any issues in `docs/postmortems/2026-05-10-first-launch.md` even if trivial

---

## Priority Stack (Next 3 Things)

After Round 2 lands:

1. **Verify accent bug fix with real Korean voice.** If this isn't actually resolved, nothing else matters for May 10 credibility.
2. **Apply D1 migrations to prod + production Google OAuth setup.** Auth won't work without these. Hard blocker for any prod login.
3. **First end-to-end prod test with Grip creds.** Everything else is theory until this runs successfully.

Everything beyond these three is secondary. These three are the chain that
unlocks every subsequent test.

---

## Open Questions (Decide Before Launch)

Small UX calls that don't block engineering but should be answered
before rehearsal.

- **Onboarding step 3** — if user picks "Chinese" as default target, does
  that auto-select Grip as destination, or let them pick platform
  independently? Recommendation: independent. Picking target language
  sets the per-stream delay default, nothing else.
- **Session timeout** — if a user signs in, creates a session, never goes
  live, does the session auto-expire? How long? Recommendation: 24hrs,
  transition to `status = 'abandoned'`.
- **Billing round-up** — bill `output_seconds / 60` rounded up to the
  nearest minute, or fractional? Recommendation: round up per-stream,
  matches how telephony bills and feels fair.
- **Stream recording retention** — `SessionRecorder` writes `video.fmp4` +
  `host_audio.pcm`. How long do we keep them? Recommendation: 30 days
  free, longer for paid Tier 4 dubbing later.

---

## Collaborators + Roles

| Role | Who | Scope | Availability |
|---|---|---|---|
| Tech / CTO | Aziz | All engineering, infra, ops, on-call | full-time |
| Sales lead | Simon | B2B clients, pricing, demos | weekday business hours |
| Client acquisition + Korean test voice | MJ | Deal closing, account management, stand-in host for dry runs (native Korean) | always available |
| First live show host | Gitae | Broadcast talent, Korean | weekends |
| Future show hosts | Yuna, others TBD | Broadcast talent | TBD |
| Videographer | TBD per show | Studio setup for live | per-show |

MJ being always-available + native Korean is a force multiplier for
testing. Accent-bug verification, 40-min endurance, voice-clone QA all
get done with MJ on weekdays. Gitae's weekend slot is reserved for
final validation + first-live-show prep, not debugging.

Aziz is the only engineer. Scale response + incident response = one person
until the company hires. Stay disciplined about what to take on.

---

## Change Log

- `2026-04-19` — File created. Consolidates vision + 5 product decisions + 21-day roadmap.
- `2026-04-19` — Added "What's Intentionally Not In The Product" section.
  Brivva serves broadcasters only, not end-audiences; streams out to
  YouTube/Grip/TikTok/etc. via RTMP, audience watches there. Home page
  `Quick Session` button + `Audience Resume Session` input are demo-era
  cruft, slated for removal in next frontend pass after coverage agents
  land.
