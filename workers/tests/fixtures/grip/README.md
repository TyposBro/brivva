# Grip Seller API Fixtures (§0.5.1)

## Blocked on vendor — all fixtures are HAND_CRAFTED_PENDING_REAL_CAPTURE

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
