# Grip Seller API Fixtures (§0.5.1)

## Status (2026-04-20) — all fixtures HAND_CRAFTED, blocked on vendor spec

| File | Status | Shape source |
|------|--------|--------------|
| `provision_stream_happy.json` | HAND_CRAFTED_PENDING_REAL_CAPTURE | Matches current stub parser in `src/features/grip/seller-api.ts` (`id` + `ingest.url` + `ingest.stream_key`). Will change when Grip ships spec. |
| `provision_stream_401_invalid_auth.json` | HAND_CRAFTED_PENDING_REAL_CAPTURE | Korean error message shape observed on Seller Center UI; spec-accurate body pending. |

Roundtrip coverage: `workers/tests/fixture-roundtrip.test.ts` drives
both through `provisionBroadcast`. Test file has a clear
`HAND_CRAFTED_PENDING_REAL_CAPTURE` tag so the invariant-regeneration
requirement survives code search.

When Grip support (`seller_support@gripcorp.co` / `cloud.bd@gripcorp.co`)
delivers the spec: regenerate both fixtures from live capture, update
`seller-api.ts` parser to the real shape, keep the roundtrip test
passing.

Grip Seller API (AccessKey + SecretKey) was discovered 2026-04-19. API spec
request is pending with `seller_support@gripcorp.co`. Until Grip provides
docs, fixture shapes are guessed from the reverse-engineered path.

Required captures once spec lands:

- `provision_stream_happy.json` — broadcast + ingest URL provision response
- `provision_stream_404_missing_product.json` — invalid product_id
- `provision_stream_401_invalid_auth.json` — AccessKey/SecretKey mismatch
- `broadcast_state_live.json` — status polling response mid-stream
- `broadcast_state_ended.json` — post-stream teardown response

## Anti-patterns to scrub once real capture arrives

- Placeholder `bcast_abc` IDs — Grip format unknown; real format TBD
- Placeholder `sk_live_xxx` stream keys — AWS IVS keys are session-auth,
  32+ chars, rotate per session
- `rtmpUrl` field name is guessed; real response may use `ingest_url`,
  `rtmp_endpoint`, or nested `cdn.ingest_address`

## Alternate path: capture via proxy

If Grip support is slow, set up an HTTPS MITM proxy (mitmproxy) between
brivva and `api.gripcorp.co`, trigger a real test broadcast, dump request
+ response pairs as fixtures. Remember to redact AccessKey/SecretKey from
committed JSON — replace with `"{{ACCESS_KEY}}"` placeholder.
