# Brivva Tech — Product Vision & May 10 Roadmap

Last updated: 2026-04-26 (WebRTC-first video plan; captions removed from product scope)
Owner: Aziz (tech) + Simon (sales) + MJ (client acquisition)
First live show: `2026-05-10`

**Agent task prompts for remaining P0/P1/P2 work live in
[`docs/may10-agent-tasks.md`](./docs/may10-agent-tasks.md).**

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
   │     zh → Grip                       (defaults: delay=3000ms  gain=0.2)
   │     ja → TikTok                     (defaults: delay=1500ms  gain=0.2)
   │     Passthrough (source) → YouTube  (delay=0ms  gain=1.0, STT/translate/TTS bypassed)
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
  "ko→ko":   { delay_ms: 0, host_gain: 1.0 },  // source==target — implicit passthrough
  "*→pass":  { delay_ms: 0, host_gain: 1.0 },  // explicit "Passthrough (source)" pick
  "_default": { delay_ms: 2500, host_gain: 0.2 },
};
```

### Passthrough destinations

The destination language dropdown includes an explicit **"Passthrough
(source)"** option (wire code `"pass"`). Picking it tells Fargate to
bypass STT + translate + TTS entirely for that stream — host audio and
video RTMP'd through at `host_gain = 1.0`, no ElevenLabs calls, no
billing for output minutes (the stream never
produces translated output).

Source-lang-equals-target-lang is the *implicit* passthrough path
(historical behaviour). Passthrough-as-destination-choice is the
*explicit* path — present since a host may want a Korean destination
with untouched audio even when their session targets are Japanese +
Chinese (and Korean isn't in `target_langs`).

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

### 6. No Captions Or Subtitles

Brivva's product is **translated voice**, not translated text. Do not add
native captions, closed captions, burned-in subtitles, or platform-specific
caption integrations to the product roadmap unless a paying B2B customer
explicitly demands it and accepts the tradeoffs.

Reasoning:
- The premium experience target is smooth 4K/30 video with natural cloned
  speech. Caption overlays compete with that goal because burned-in text
  forces extra per-language video rendering and encoding.
- Platform support is uneven. YouTube supports live captions, AWS IVS supports
  caption ingest, but TikTok/Instagram public RTMP support is unclear or
  unavailable. Grip still needs platform-side confirmation.
- CJK languages are the core launch use case, and legacy caption standards
  are a poor fit. CEA-608 cannot represent Korean/Chinese/Japanese; CEA-708
  support depends on the platform/player and is not reliable enough to anchor
  the product.
- Native captions would create a fragmented UX: some destinations get
  selectable captions, others require burn-in, and multi-language caption
  support is often limited to one track. That complexity does not improve the
  core promise: shoppers hear the creator in their own language.

Implementation direction:
- Prioritize WebRTC video ingest so the browser sends real encoded video
  instead of JPEG frames over WebSocket.
- Keep RTMP outputs focused on video + translated/cloned audio.
- If a platform or customer later requires text, treat it as a separate
  paid/enterprise feature, not part of May 10 or the default product.

---

## What's In Scope For May 10

### Already Shipped

| Layer | What |
|---|---|
| server-rs | Clean 4-layer architecture, WebSocket + FFmpeg pipeline, per-language video delay, audio mixing (host/TTS), source-lang passthrough, 2 kill-switches wired (`BRIVVA_FALLBACK_TO_DEFAULT_VOICE`, `BRIVVA_FORCE_RTMP_NOT_RTMPS`), RTMPS verified for Grip, idle-restart for Grip regional drops, SessionMetrics export task, kill-switch integration tests, ffmpeg compiled with `--enable-librtmp --enable-openssl` (required for AWS IVS / Grip acceptance, verified 2026-04-20) |
| workers | D1 schema (users, voices, sessions, streams, platform_credentials, session_metrics), D1 migration `0008_user_onboarding_billing.sql` (active_voice_id, onboarding_completed_at, billing_tier, bills_to), Google OAuth for sign-in + YouTube OAuth for add-channel, Grip/TikTok credential paste endpoints, voice clone with source_lang language labels, WAV duration validation (30s min, 3min max), `POST /api/user/complete-onboarding`, `POST /api/voices` upsert (deletes prior clone + updates active_voice_id pointer), `GET /api/billing/rate`, `POST /api/sessions/:id/quote`, `GET /api/sessions/:id/summary`, Stripe webhook signature verifier stub |
| frontend | In-memory JWT auth, SignInGate route guard, `/onboarding` 3-step wizard (platform/voice/default-lang) with route guard on `onboarding_completed_at`, voice recorder 30s–180s with progress bar, `stream-defaults.ts` table (ko→zh/ja/en covered; th/vi/id still missing), pre-stream quote modal with 10–180min duration slider, post-stream summary modal, singular "Your voice" + "Re-record" UX, collapsed Advanced toggle (delay+gain) on destination card, host-page split (/setup + /live), session-page observability strip (live minutes + cost estimate), dashboard with platform region grouping |
| infra | Terraform ECR + ECS + IAM + CloudWatch + Secrets Manager, GitHub OIDC deploy, SHA-pinned images, ECS deployment circuit breaker, cargo-zigbuild cross-compile (5min builds), Dockerfile CI guard asserts `ffmpeg -version | grep enable-librtmp` (prevents silent librtmp regression on base-image bump), `./scripts/rollback.sh`, `./scripts/smoke-test.sh` |
| CI/CD | Contracts drift check, server-rs + workers + frontend typecheck/test/build, layer audits (0 violations across all 3), pre-commit + pre-push + post-merge git hooks |
| docs | ARCHITECTURE.md, runbook.md, grip-integration-notes.md, terraform-state-migration-plan.md, this file |

### Round 2 Remaining (audit 2026-04-20 — LANDED same day)

All four Round 2 rows shipped in commit `a74993d` on 2026-04-20:
`/quote` breakdown, `/summary` reshape + B2B variant, SEA defaults
(ko→th/vi/id), and `active_voice_id` dispatch with a new
`session_ws/active_voice_refresh.rs` periodic task so re-records
retarget TTS mid-session without a restart.

Prompt history preserved in
[`docs/may10-agent-tasks.md`](./may10-agent-tasks.md) under Tasks 1-4.

| Agent | Task | Why |
|---|---|---|
| Workers | `POST /api/sessions/:id/quote` — add per-lang breakdown to the response | Frontend `quote-api.ts` expects a `breakdown` field; current response only has aggregate `output_minutes` + `estimated_cost_usd`. Pre-stream modal can't itemize langs. |
| Workers | `GET /api/sessions/:id/summary` — return `total_minutes` + `total_cost_usd` shape, add B2B variant (`billed_to` field populated when `users.billing_tier = 'b2b'`) | Frontend `summary-modal.tsx` keys off `total_minutes` / `total_cost_usd`, not `source_minutes` / `output_minutes_by_lang`. B2B "Billed to Simon" line is spec'd in §Pricing UX but never wired. |
| Frontend | Extend `stream-defaults.ts` with `ko→th`, `ko→vi`, `ko→id` (all `delay_ms: 2500`, `host_gain: 0.2`) | Table currently covers only ko→zh/ja/en/ko + `*→pass` + `_default`. SEA expansion is in scope per §Unit Economics; defaults must exist before first SEA stream. |
| server-rs | Regen contracts to pick up `users.active_voice_id`; swap session dispatch from `session.voice_id` → `user.active_voice_id` | Workers now upserts the pointer but server-rs still reads `session.voice_id`. One-voice-per-user invariant (§Product Decision 3) isn't enforced at dispatch until this lands. |

### Round 3 Candidates (if endurance test surfaces bugs)

- Indian-accent regression if voice_settings + source_lang hint doesn't actually fix it — fallback is `eleven_multilingual_v2_5` model_id or strict enrollment-lang mismatch forcing default voice
- Chinese voice quality if default library voice sounds robotic — pick better voice IDs
- Concurrent session handling if ECS scaling is flaky at 2-3 parallel streams

### Testing Debt (audit 2026-04-20 — resolved same day)

Five §0 violations surfaced by an audit of `claude.md` §0 against the
codebase. P1 + P2 multi-step + P3 e2e landed the same day (below);
one P2 row (concurrent-connect race) is deferred to post-May-10 with
the fix recipe documented inline.

| Priority | Gap | Rule | Status | Evidence |
|---|---|---|---|---|
| **P1a** | Real-response fixtures for every vendor (ElevenLabs, Stripe, YouTube, Google OAuth, Grip). | §0.5.1 | **LANDED 2026-04-20.** 14 fixture JSONs under `workers/tests/fixtures/<vendor>/`; 15 roundtrip tests in `workers/tests/fixture-roundtrip.test.ts` drive each through the real client deserializer. Hand-crafted pending real capture, provenance marked per vendor README. | `workers/tests/fixture-roundtrip.test.ts`, `workers/tests/fixtures/{elevenlabs,stripe,youtube,google-oauth,grip}/` |
| **P1b** | Silent paths emit zero structured logs. STT reconnect skip, workers credential/OAuth fallbacks, frontend JSON.parse catches — all silent. | §0.5.4 | **LANDED 2026-04-20.** Every silent `continue` / try_send drop / catch-fallback in the STT pipeline, workers orchestration, and frontend session pages now emits structured `tracing::warn!` / `console.warn` with `session_id` / `tag` / `error` context. | `server-rs/src/features/broadcast/data/pipeline/{stt,stt_response,stt_transport}.rs`, `server-rs/src/features/broadcast/data/session_ws/messages.rs`, `workers/src/{orchestration/app.ts,features/billing/stripe-webhook.ts}`, `frontend/src/features/broadcast/presentation/{session-setup-page,session-page,onboarding-page,dashboard-page}.tsx` |
| **P2a** | WS lifecycle matrix — missing crash→restart + concurrent start×2. | §0.5.2 | **PARTIAL 2026-04-20.** `ws_abrupt_drop_without_host_end_cleans_live_session` passes (crash-drop recovery). `concurrent_connects_for_same_session_converge_to_single_live_session` is `#[ignore]`'d — it exposes a real race in `handle_host`: `evict_stale_live_sessions` runs BEFORE `live_sessions.insert`, so two parallel handlers can both observe an empty map and both insert. **Fix (post-May-10):** add a per-session_id `tokio::sync::Mutex` to `BroadcastState`, acquire before eviction, hold through insert. Deferred because FE never fires concurrent connects by design; sequential stop→start (the actual 2026-04-20 incident shape) is covered. | `server-rs/tests/e2e_live_session_lifecycle.rs` |
| **P2b** | Multi-step integration chains (create → mid-flight mutation → background loop handles transition). | §0.5.3 | **LANDED 2026-04-20.** `workers/tests/multi-step-integration.test.ts` chains create → add-stream → go-live → mid-live add-stream → two metrics PATCHes → soft-end → post-end metrics → final usage rollup. Covers the exact race shape behind the 2026-04-20 triad. | `workers/tests/multi-step-integration.test.ts` |
| **P3** | E2E voice-clone cross-lingual chain — proves UI → Workers half of the invariant even though the TTS wire body stays a server-rs integration concern. | §0.2 (voice-clone row = Unit + Integration + e2e) | **LANDED 2026-04-20.** `frontend/e2e/voice-clone-crosslingual.e2e.ts` — Korean-enrolled voice + Japanese destination asserts source_lang + target_langs in the POST /api/sessions payload without tripping the mismatch banner; a second test proves the banner RE-engages when the voice is re-recorded in a new language. | `frontend/e2e/voice-clone-crosslingual.e2e.ts` |

**Also outstanding: the §0.6 pre-merge gate itself is not enforced in
CI.** No `.github/workflows/` wires the 8-step gate. Recent fixes
(`b960367`, `b3542f4`, `3055ccc`) did add tests, but that's discipline,
not automation. Any PR today can skip §0.5.1–4 and land. Add the gate
to CI before self-serve onboarding opens (post-May-10); not a May 10
blocker but a Phase 2 blocker.

**Remaining follow-ups after this pass:**
- **§0.5.4 post-merge log-audit** — rule demands a 30-min session run
  with grep over every silent branch in the log output, not just
  "wired." Logs are wired but verification didn't happen. Prompt in
  `docs/may10-agent-tasks.md` Task 17. Complete before endurance test.
- **Capture real vendor responses** to replace `HAND_CRAFTED_PENDING_REAL_CAPTURE`
  fixtures for Soniox + YouTube + Grip. Prompt in
  `docs/may10-agent-tasks.md` Task 16. Each per-vendor README has exact
  capture steps. Stripe deferred until pricing lands.
- Convert `tts.rs` + `ffmpeg/{mod,drain}.rs` `eprintln!` calls
  to `tracing::*` for structured logging. Not silent paths — they emit —
  but the rule requires structured output. Out of scope for the
  silent-path pass; batch into a separate observability cleanup.
- Fix the concurrent-connect race once May 10 launch is behind us.
  Recipe: per-`session_id` `tokio::sync::Mutex` on `BroadcastState`,
  acquire before `evict_stale_live_sessions`, hold through
  `live_sessions.insert`.

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

- **Stripe real billing** — B2B doesn't need it; self-serve is post-launch (committed Phase 2)
- **Tier 4 post-processed VOD** — no current customer demand
- **Admin dashboard for Simon** — direct D1 queries work for May 10. Committed Phase 2 (promised to Simon + Yuni 2026-04-20 in WhatsApp: "after May-10th I will build B2B admin panel so you guys can onboard clients more smoothly")
- **Multi-speaker support** — single-speaker only for May 10. Committed Phase 2 (promised 2026-04-20 alongside SEA + Stripe)
- **Multi-region Fargate** — us-east-1 only; performance is fine for Korean + SEA
- **Terraform S3 state cutover** — scheduled post-May-10 (infrastructure prepped, disabled pending drift resolution)
- **Naver OAuth / Apple Sign-In** — Google only until demand proves otherwise
- **Instagram integration** — Grip + TikTok + YouTube cover the primary market
- **Captions / subtitles** — removed from product scope. Brivva sells natural
  translated voice, not text overlays. Reconsider only as a paid enterprise
  request.
- **Mobile apps** — web-only for launch
- **Per-language pricing** — flat `$1.50/min` across all languages for launch
- **Incorporation details** — Brivva Tech entity formation (34% equity push) is parallel, not blocking

### Committed Phase 2 (post-May-10)

Scope promised externally. Cannot quietly drop without pushback.

| Commitment | Promised to | When | Source |
|---|---|---|---|
| B2B admin panel (Simon + Yuni can onboard clients without Aziz) | Simon, Yuni | 2026-04-20 | WhatsApp: "after May-10th I will build B2B admin panel" |
| SEA language expansion (Thai / Vietnamese / Indonesian) | Simon | 2026-04-20 | WhatsApp: "once it succeeds, I will work on adding SEA languages" |
| Multi-speaker support | Simon | 2026-04-20 | WhatsApp: "multi-speaker support". **Implementation:** Soniox now supports multi-speaker diarization + translation natively. Delta is on our side: one TTS WebSocket connection per speaker, each with its own `voice_id`. Requires per-speaker voice enrollment in onboarding and a `speaker_id → voice_id` routing map in server-rs dispatch. ElevenLabs rate / quota scales linearly with speaker count — verify before multi-speaker shows. |
| Stripe self-serve billing | Simon | 2026-04-20 | WhatsApp: "Stripe integration" |

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

**"Ultimate test" (promised 2026-04-20):** Aziz's cloned voice,
English source → Korean + Chinese + Japanese targets simultaneously,
broadcasting to Grip + TikTok + YouTube simultaneously. This is the
public commitment to Simon + Yuni. Native-speaker sign-off required on
all three target langs (MJ for KR, Yuni for JP, default-library voice
QA for ZH). Exercises the 3-lang × 3-platform concurrent-stream path
and flips source to English (inverse of today's En→Ja single demo).

- First full dry run with shared Grip + TikTok creds (MJ speaks Korean)
- Stream Korean → Chinese (Grip) + Japanese (TikTok) for 40 minutes
- Document every bug, fix criticals same-day
- Lock in best-sounding default Chinese voices (pick from ElevenLabs library)
- Concurrent session test: 2 browsers, 2 hosts, simultaneous streams
- Monitoring/alerting check: does a CloudWatch alarm actually wake Aziz?
- Evaluate Grip official Seller API (AccessKey/SecretKey, discovered
  2026-04-19 on Seller Center → profile → "Grip API (외부 연동)",
  support: `seller_support@gripcorp.co`). If coverage is sufficient,
  most reverse-engineered workarounds in `docs/grip-integration-notes.md`
  can retire. Gate rollout behind a `BRIVVA_GRIP_USE_LEGACY` kill-switch
  so paste-cred path remains instant-rollback. Not on the critical path
  for May 10 — current paste-cred + librtmp flow is working.

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
| Japanese-language QA | Yuni | Native-JP review of translation + voice quality. Validated 2026-04-20 En→Ja demo quality. | on-demand |
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
- `2026-04-20` — Codebase audit: Round 2 largely shipped (onboarding wizard,
  stream-defaults, quote/summary modals, "Your voice" UX, Advanced toggle,
  D1 migration 0008, `/api/user/complete-onboarding`, `/api/voices` upsert,
  `/api/billing/rate`, `/api/sessions/:id/quote`+`summary`,
  `docs/b2b-onboarding-notes.md`). Moved shipped rows into "Already
  Shipped". Remaining gaps: quote/summary response-shape mismatch with
  frontend (`breakdown` + `total_minutes`/`total_cost_usd` + B2B "Billed
  to" variant), SEA targets missing from `stream-defaults.ts`, server-rs
  contract regen + dispatch swap to `user.active_voice_id`.
- `2026-04-20` — Confirmed ffmpeg-with-librtmp is mandatory for AWS IVS
  (Grip backend) — native RTMP silently truncates at frame 3. Dockerfile
  compiles ffmpeg from source with `--enable-librtmp --enable-openssl`
  and CI asserts the flag. Verified end-to-end (600 frames in 20s,
  broadcast flipped to `방송중`).
- `2026-04-20` — Grip has an official Seller API (AccessKey/SecretKey)
  discovered on Seller Center. Added Week 2 evaluation task behind
  `BRIVVA_GRIP_USE_LEGACY` kill-switch; not on the critical path for
  May 10, but could retire most reverse-engineered Grip workarounds
  post-launch.
- `2026-04-20` — Testing-debt audit vs `claude.md` §0. Five gaps
  surfaced: no real-response fixtures (§0.5.1), silent-path logs
  missing in STT pipeline (§0.5.4), WS lifecycle matrix
  missing crash/restart + concurrent-start rows (§0.5.2), multi-step
  integration chains thin (§0.5.3), voice-clone e2e split across
  files (§0.2). P1 gaps are the exact class that shipped the April
  2026 triad. Also: §0.6 pre-merge gate not CI-enforced; tracked as
  Phase 2 blocker.
- `2026-04-20` — Testing-debt resolved in same-day sprint. 4 of 5
  gaps landed; 1 (concurrent-connect race) is `#[ignore]`'d with
  an inline fix recipe and deferred to post-May-10. Ship totals:
  - P1a fixtures: 14 vendor fixtures + 15 roundtrip tests in
    `workers/tests/fixture-roundtrip.test.ts` driving real client
    deserializers (ElevenLabs cloneVoice, Stripe HMAC verify,
    YouTube createBroadcast, Google OAuth exchange+userinfo+refresh,
    Grip provisionBroadcast).
  - P1b silent paths: `tracing::*` added to every silent STT
    reconnect / token drop / session-gone return across
    `stt{,_response,_transport}.rs` + `session_ws/messages.rs`;
    `console.warn` added to workers `parseTargetLangs` / Stripe
    webhook JSON catch + four frontend session-page catches.
  - P2a WS lifecycle: `ws_abrupt_drop_without_host_end_cleans_live_session`
    passes; `concurrent_connects_...` ignored documenting a real
    race between eviction + insert in `handle_host`.
  - P2b multi-step chain: `workers/tests/multi-step-integration.test.ts`
    drives create → add-stream → go-live → mid-live add-stream → two
    metrics PATCHes → soft-end → post-end metrics → final usage
    rollup.
  - P3 e2e: `frontend/e2e/voice-clone-crosslingual.e2e.ts` — 2 tests
    covering Korean voice + Japanese destination happy path and the
    re-record-in-new-language banner re-engagement.
  Totals: server-rs 253 passing + 1 ignored, workers 234, frontend
  unit 234, e2e 20. All suites green.
- `2026-04-20` (evening) — Deploy shipped. SaaS live at prod URL, link
  shared in Brivva WhatsApp group, test users invited by Gmail. Grip +
  YouTube integrations verified end-to-end in prod (librtmp path
  holding). TikTok integration is the active next target; then
  onboarding / session / broadcast UX polish. Aziz emailed
  `seller_support@gripcorp.co` requesting Grip Seller API docs; reply
  pending. Two parallel agents continued closing claude.md §0.5 +
  §5.1 debt: 232 workers tests (14 fixture JSONs roundtripped),
  session_ws.rs split 1081→7 files (≤270L), ffmpeg/mod.rs split
  1465→759L + args.rs + tests.rs. All source files now under the
  §5.1 800L hard limit except `drain.rs` (656L, pre-existing).
  §0.5.4 log-audit itself (30-min grep verification) still pending —
  tracked as a follow-up in `docs/may10-agent-tasks.md` Task 17.
- `2026-04-20` (evening) — Added `docs/may10-agent-tasks.md` with
  self-contained agent prompts for every remaining P0/P1/P2 task
  surfaced in this vision. Critical-chain for May 10:
  `10 → 6/7/8 → 11 → 24 → 28`. Tasks 1/2/4 ship before 11. Tasks 15,
  21, 23 are Phase 2.
- `2026-04-20` (late evening) — Massive parallel-agent sprint
  landed the bulk of the remaining list to main:
  - `a74993d` Tasks 1/2/3/4 (all of Round 2): `/quote` breakdown
    array, `/summary` reshape + B2B variant, `stream-defaults`
    SEA rows, `active_voice_id` dispatch + mid-session refresh
    task.
  - `03b36d1` Tasks 17/16/19 infrastructure: §0.5.4 silent-path
    logs + 44-row audit checklist + runner (`scripts/post-merge-log-audit.sh`);
    §0.5.1 real-capture scripts (`scripts/capture-{soniox,youtube}-fixtures.sh`);
    §0.5.3 multi-step integration chain plus migration 0010
    unique index `streams(session_id, lang, platform)` to block
    duplicate-ffmpeg-spawn on concurrent add-stream.
  - `60bb215` ffmpeg-base prebuilt ECR image: `infra/ffmpeg-base/`
    + `.github/workflows/ffmpeg-base.yml` on native amd64 runner.
    Server-rs Dockerfile now `COPY --from=ffmpeg-base` instead of
    recompiling from source each deploy (was 2hr QEMU amd64, now
    native amd64). IAM trust policy widened AWS-side, first
    manual workflow_dispatch run succeeded 2026-04-20.
  - `db63713` Tasks 21/20/23: §0.6 pre-merge CI gate
    (`.github/workflows/pre-merge-gate.yml` + 4 delta-check
    scripts, all green on HEAD locally), full-chain cross-lang
    voice-clone e2e (`voice-clone-cross-lang-full-chain.e2e.ts`
    asserts source_lang carries through `POST /api/voices` +
    `POST /api/sessions`), and all demo-era cruft removed
    (`/host` route + `Quick Session` + `Audience Resume` gone;
    grep now zero across frontend + backend + e2e).
- `2026-04-20` (late evening) — Prod wiring complete. Wrangler
  secrets verified present (`ELEVENLABS_API_KEY`, `GOOGLE_CLIENT_ID`,
  `GOOGLE_CLIENT_SECRET`, `GRIP_{ACCESS,SECRET}_KEY`,
  `INTERNAL_SECRET`, `JWT_SECRET`; `STRIPE_WEBHOOK_SECRET` deferred
  to Phase 2). D1 prod migration 0010 applied (`wrangler d1
  migrations apply brivva --remote` — zero pre-existing dupes in
  `streams`, unique index created, applied migrations now
  0001-0010). Google OAuth redirect URIs confirmed registered by
  Aziz. Accent-bug fix confirmed by Aziz. Critical chain collapse:
  Tasks 6/7/8/10 all ✅ — next blocker is Task 11 (40-min endurance
  with MJ).
- `2026-04-20` (evening) — Aziz shared En→Ja YouTube demo in Brivva
  WhatsApp group. Yuni (native JP, new QA resource) approved quality
  for the Japanese output. Simon asked for Korean next. Aziz committed publicly
  to: (a) "ultimate test" = En source → Ko+Zh+Ja targets
  simultaneously → Grip+TikTok+YouTube simultaneously, cloning own
  voice, (b) Phase 2 scope of SEA langs + multi-speaker support +
  Stripe, (c) B2B admin panel post-May-10 so Simon + Yuni onboard
  clients without Aziz in the loop. Added Yuni to Collaborators table,
  moved admin panel + multi-speaker from "Not In Scope" to new
  "Committed Phase 2" table, wrote the ultimate-test brief into Week 2.
- `2026-04-26` — Captions/subtitles removed from product scope. Rationale:
  Brivva's core premium experience is smooth video plus natural cloned
  translated voice; captions fragment across platforms, CJK support is weak
  in legacy caption standards, and burn-in adds per-language video encode
  cost that fights 4K/30 quality. WebRTC ingest is the video priority.
- `2026-04-26` — WebRTC ingest made the frontend default. Legacy JPEG
  websocket video remains as an explicit fallback with `?ingest=jpeg` or
  `VITE_VIDEO_INGEST=jpeg` while production streams are validated.
