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
