# ElevenLabs Fixtures (§0.5.1)

## Pending — all fixtures are HAND_CRAFTED_PENDING_REAL_CAPTURE

- `voice_clone_happy.json` — POST /v1/voices/add response
- `voice_clone_402_quota.json` — quota-exceeded error body
- `tts_stream_headers.json` — TTS stream response headers (for format
  validation)
- `voice_delete_happy.json` — DELETE /v1/voices/:id response

## Capture steps

```bash
curl -X POST https://api.elevenlabs.io/v1/voices/add \
  -H "xi-api-key: $ELEVENLABS_API_KEY" \
  -F "name=capture-$(date +%s)" \
  -F "files=@sample.m4a" | tee voice_clone_happy.json
```

For 402: provision a trial key that has exhausted quota and issue the same
request. Real ElevenLabs voice IDs are 20-char alphanumeric — validate
fixture matches format before committing.

## Known anti-patterns to avoid

- Inline `{ voice_id: "el-1" }` — real IDs are `pNInz6obpgDQGcFmaJgB`-length
- Inline quota error as free-form string — real body is structured JSON
- Hardcoded `expires_in` — ElevenLabs doesn't expose token expiry this way
