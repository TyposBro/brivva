# Google OAuth Fixtures (§0.5.1)

Covers both the sign-in flow (openid scope) and the YouTube OAuth flow
(youtube scope). They share the token endpoint but differ in scopes and
userinfo call patterns.

## Status (2026-04-20)

| File | Status | Source |
|------|--------|--------|
| `signin_token_exchange.json` | HAND_CRAFTED_PENDING_REAL_CAPTURE | ya29.* access_token + JWT id_token + 3599 expires_in per Google docs |
| `signin_userinfo.json` | HAND_CRAFTED_PENDING_REAL_CAPTURE | Real shape including locale + email_verified |
| `signin_userinfo_missing_picture.json` | HAND_CRAFTED_PENDING_REAL_CAPTURE | Variant: user without profile photo — exercises `picture: null` normalization |
| `youtube_token_exchange.json` | HAND_CRAFTED_PENDING_REAL_CAPTURE | refresh_token with 1// prefix; space-separated scope |
| `invalid_grant.json` | HAND_CRAFTED_PENDING_REAL_CAPTURE | 400 error envelope from revoked refresh_token |

Roundtrip coverage: `workers/tests/fixture-roundtrip.test.ts` drives
each through `exchangeCode` (sign-in + youtube) / `fetchUserInfo` /
`refreshAccessToken`.

## Still to add

- `youtube_refresh_happy.json` — refresh_token flow (youtube scope)
- `invalid_client.json` — wrong client_id/secret pairing

## Capture steps

Real Google accounts only. Test via OAuth Playground
(developers.google.com/oauthplayground) — pick scopes, authorize, exchange
code, copy token + userinfo responses.

```bash
curl -X POST https://oauth2.googleapis.com/token \
  -d "code=$CODE" \
  -d "client_id=$CLIENT_ID" \
  -d "client_secret=$CLIENT_SECRET" \
  -d "redirect_uri=$REDIRECT_URI" \
  -d "grant_type=authorization_code" | tee signin_token_exchange.json
```

## Validation rules

- `access_token` starts with `ya29.`
- `refresh_token` starts with `1//`
- `id_token` is a JWT — 3 dot-separated base64 segments
- `expires_in` is 3599 (Google's 1s shave)
- `scope` is space-separated, NOT comma-separated (easy mistake in mocks)
