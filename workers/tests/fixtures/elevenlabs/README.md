# ElevenLabs Fixtures (§0.5.1)

## Status (2026-04-20)

| File | Status | Source |
|------|--------|--------|
| `voice_clone_happy.json` | HAND_CRAFTED_PENDING_REAL_CAPTURE | Shape per https://docs.elevenlabs.io/api-reference/voices/add — 20-char alphanumeric voice_id |
| `voice_clone_402_quota.json` | HAND_CRAFTED_PENDING_REAL_CAPTURE | `{ detail: { status, message } }` — replace with live-captured body when a test key hits the cap |
| `voice_delete_happy.json` | HAND_CRAFTED_PENDING_REAL_CAPTURE | `{ status: "ok" }` per docs |

Roundtrip coverage: `workers/tests/fixture-roundtrip.test.ts` drives
each fixture through `cloneVoice` / `deleteRemoteVoice`.

## Still to add

- `tts_stream_headers.json` — TTS stream response headers (for format validation)

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
