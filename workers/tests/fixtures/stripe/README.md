# Stripe Fixtures (§0.5.1)

## Pending — all fixtures are HAND_CRAFTED_PENDING_REAL_CAPTURE

Webhook signature verification is tested without real event payloads. That
leaves tier updates, customer metadata, and nested subscription shape
unvalidated. These are Phase 2 (post-May-10) but the fixture dir is
scaffolded now so capture can happen without infra work.

Required captures:

- `checkout_session_completed.json` — full event JSON from a real test-mode
  checkout completion
- `invoice_paid.json` — subscription renewal webhook
- `customer_subscription_deleted.json` — cancellation webhook
- `invoice_payment_failed.json` — dunning trigger
- `checkout_session_expired.json` — abandoned-cart path

## Capture steps

Stripe Dashboard → Developers → Webhooks → create endpoint (can be
request.bin / webhook.site for capture-only). Trigger events via Stripe
CLI:

```bash
stripe trigger checkout.session.completed
stripe trigger invoice.paid
```

Copy full JSON payload (NOT just the `data.object` portion — we need the
envelope with `id`, `type`, `api_version`, `created`, `livemode`).

## Validation rules

- IDs match Stripe prefixes: `cs_test_...`, `evt_...`, `sub_...`, `cus_...`
- `livemode: false` for test-mode fixtures (asserted in loader)
- Timestamps are unix seconds (integer), not ISO strings
- `api_version` matches what our code expects
