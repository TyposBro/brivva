# YouTube Live API Fixtures (§0.5.1)

## Status (2026-04-20)

| File | Status | Source |
|------|--------|--------|
| `broadcast_insert_happy.json` | HAND_CRAFTED_PENDING_REAL_CAPTURE | Shape per https://developers.google.com/youtube/v3/live/docs/liveBroadcasts — 11-char URL-safe id |
| `stream_insert_happy.json` | HAND_CRAFTED_PENDING_REAL_CAPTURE | Shape per liveStreams docs; exercises `cdn.ingestionInfo.ingestionAddress` + `streamName` parse path |
| `broadcast_403_quota.json` | HAND_CRAFTED_PENDING_REAL_CAPTURE | Google API error envelope; drives `YouTubeBroadcastError` surfacing |

Roundtrip coverage: `workers/tests/fixture-roundtrip.test.ts` drives
all three through `createYouTubeBroadcast`.

## Still to add

Current tests use `bcast-123` / `stream-456`. Real YouTube broadcast and
stream IDs are 11-char alphanumeric (URL-safe base64). If parsing code
ever assumes format, placeholder IDs let bugs pass tests but break prod.

Required captures:

- `broadcast_insert_happy.json` — POST /liveBroadcasts response
- `stream_insert_happy.json` — POST /liveStreams response
- `bind_happy.json` — POST /liveBroadcasts/bind response
- `broadcast_403_quota.json` — daily quota exceeded
- `oauth_token_exchange.json` — POST /oauth2/v4/token response
- `oauth_refresh_happy.json` — refresh_token flow
- `oauth_invalid_grant.json` — revoked refresh_token error

## Capture steps

Use a test Google account, not the prod brand channel. `curl` against
YouTube Data API v3 with scope `https://www.googleapis.com/auth/youtube`.

```bash
curl -X POST "https://www.googleapis.com/youtube/v3/liveBroadcasts?part=snippet,contentDetails,status" \
  -H "Authorization: Bearer $ACCESS_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"snippet":{"title":"capture","scheduledStartTime":"2026-05-01T00:00:00Z"},"status":{"privacyStatus":"unlisted"}}' \
  | tee broadcast_insert_happy.json
```

## Validation rules

- Broadcast IDs: 11-char `[A-Za-z0-9_-]`
- `lifeCycleStatus` enum: `created`, `ready`, `testStarting`, `testing`,
  `liveStarting`, `live`, `complete`, `revoked`
- OAuth tokens: real ya29.* access tokens, 1/... refresh tokens
- `expires_in` is 3599 (not round 3600) — Google shaves 1s
