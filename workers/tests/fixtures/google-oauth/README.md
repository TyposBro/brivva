# Google OAuth Fixtures (§0.5.1)

Covers both the sign-in flow (openid scope) and the YouTube OAuth flow
(youtube scope). They share the token endpoint but differ in scopes and
userinfo call patterns.

## Pending — all fixtures are HAND_CRAFTED_PENDING_REAL_CAPTURE

- `signin_token_exchange.json` — POST /oauth2/v4/token with openid scope
- `signin_userinfo.json` — GET /oauth2/v3/userinfo response
- `signin_userinfo_missing_picture.json` — user without profile photo
- `youtube_token_exchange.json` — same endpoint, youtube scope
- `youtube_refresh_happy.json` — refresh_token flow
- `invalid_grant.json` — revoked refresh_token 400 response
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
