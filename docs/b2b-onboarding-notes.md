# B2B Onboarding — Ops Runbook

Self-serve is the default billing path. B2B clients (agencies, studios,
anyone invoiced on a contract) get flagged manually by Simon after the
sales handoff. This file is the copy-paste workflow.

## What the flag controls

`users.billing_tier = 'b2b'` is consumed by:

- Future pricing logic — B2B contract rate supersedes the self-serve
  `per_output_minute_usd` returned by `GET /api/billing/rate`. Today both
  tiers see the same rate, but the discriminator is in place.
- Stripe webhook handling — B2B charges go through an invoice flow, not
  the self-serve subscription flow (code for this lands with real billing).
- Support triage — `/api/user` returns `billing_tier` so FE renders a
  "Contact your account manager" banner instead of an upgrade CTA.

`users.bills_to` is free-text metadata: company name, PO number, or the
AP contact's email. It exists so Finance has a breadcrumb without needing
to open the CRM.

## Flagging a B2B client

Get the user id first — easiest is `SELECT id, email FROM users WHERE
email = '<their-signin-email>'`.

```bash
cd workers

# Confirm the user exists + is currently on self-serve.
bunx wrangler d1 execute brivva --remote \
  --command="SELECT id, email, billing_tier, bills_to FROM users WHERE id = '<USER_ID>'"

# Flip to B2B with an invoice target.
bunx wrangler d1 execute brivva --remote \
  --command="UPDATE users SET billing_tier='b2b', bills_to='Acme Corp (ap@acme.co)' WHERE id = '<USER_ID>'"

# Verify.
bunx wrangler d1 execute brivva --remote \
  --command="SELECT id, email, billing_tier, bills_to FROM users WHERE id = '<USER_ID>'"
```

Drop `--remote` to rehearse on the local miniflare D1 first.

## Rolling back

```bash
bunx wrangler d1 execute brivva --remote \
  --command="UPDATE users SET billing_tier='self_serve', bills_to=NULL WHERE id = '<USER_ID>'"
```

## What NOT to do

- Don't edit `billing_tier` via the API. There's no endpoint; that's
  intentional (the flag is a business decision, not a user action).
- Don't mix `billing_tier='b2b'` with a Stripe subscription id on the
  same row. B2B clients are not in Stripe; double-billing is the worst
  outcome.
- Don't flag trial accounts as B2B. Wait until the contract is signed —
  the flag is load-bearing for invoicing.

## Grip destination — getting stream key + URL

Grip has no public Seller API (docs gated behind cloud.bd@gripcorp.co,
email sent 2026-04-19). Until they reply, the prod path is paste-creds:
host copies stream key + URL from Grip Business Center into Brivva's
destination card. Walk new B2B hosts through this the day of broadcast.

### Steps (for Simon / MJ to share with the host)

1. Sign in to Grip Business Center → `seller.grip.show`
2. Left nav: `채널 관리` (Channel Mgmt) → `방송 관리` (Broadcast Mgmt) → `방송 목록` (Broadcast List)
3. Click `PC 송출` (PC Broadcast) on the row for today's show
4. Fill broadcast info (title, thumbnail, scheduled start time)
5. Set `송출 종류` (broadcast type) to **`라이브`** (Live). Use **`녹화`** (Recording) only for private rehearsal
6. Click `저장` (Save) — the server URL + stream key appear in the third section of the form
7. Copy both → paste into the Brivva destination card (Server URL + Stream Key)
8. Click "Save credentials" in Brivva so next session pre-fills

### Timing (critical — plan the call slot around this)

- Live keys issue **1 hour before scheduled start time**. Cannot fetch earlier.
- Rehearsal keys issue **5 hours before scheduled start time**.
- Keys are **one-shot per broadcast** — a new broadcast → new keys. Don't reuse across sessions.
- After saving the broadcast, Grip requires you to **start pushing RTMP within 1 hour** or the stream is invalidated.
- The `송출 종류` (transmission type) **cannot be changed after save** — pick Live or Recording (rehearsal) correctly the first time.
- Connect-to-reservation window: **15 minutes before scheduled start time through end time**. Push outside that window and Grip won't bind the stream to the reserved broadcast slot.
- Grip terminates the broadcast after **15 minutes of network interruption** — if Fargate reconnects after that window it won't resume.

### Grip encoder constraints (Brivva ffmpeg must respect)

Per the PC 송출 form:

- Codec: **H.264**
- Keyframe interval: **1 second**
- Resolution: **720×1280** (portrait, 9:16)
- Bitrate: **≤ 3 Mbps** — exceeding this causes Grip to drop the connection mid-stream

Brivva's server-rs ffmpeg per-language output must match these. If the
existing pipeline emits 16:9 landscape or >3 Mbps, add a Grip-specific
ffmpeg profile before the first real b2b show. Check before May 10.

### Name / password fields on the form

The PC 송출 form shows `name` + `password` next to `stream key` + `server`.
Per Grip's PDF guide, **authentication is optional** ("인증 기능은 사용하지
않아도 무관"). Brivva only needs server URL + stream key — leave name /
password blank in Grip and on our side.

### When Grip answers the Seller API request

Wire the real endpoint into `workers/src/features/grip/seller-api.ts`
(see the TODO block there). Once live, Grip destinations auto-provision
on session-create, and this manual flow becomes the fallback path rather
than the primary one.
