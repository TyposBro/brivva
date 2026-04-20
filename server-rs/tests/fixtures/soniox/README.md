# Soniox Real-Response Fixtures (§0.5.1)

Fixtures captured from live Soniox STT WebSocket sessions. Do not hand-craft
shapes — append new fixtures only from real captures.

| File | Source | Status |
|------|--------|--------|
| `error_auth_failed.json` | Observed with invalid API key on `stt-rt-preview` | CAPTURED |
| `error_408_timeout.json` | April 2026 triad regression — transport timeout; `error_code` arrives as **integer** not string | CAPTURED |
| `error_absent.json` | Mid-stream happy frame with no tokens yet | CAPTURED |
| `error_null.json` | Explicit-null variant observed in connection-open frames | CAPTURED |

## Pending (§0.5.1 gaps worth filling before May 10)

- `happy_transcript_en_ja.json` — translation frame with `translation_status: "translation"`
- `happy_source_en.json` — source-mode frame with `translation_status: "original"`
- `end_token.json` — frame containing `<end>` sentinel
- `error_429_rate_limited.json` — rate-limit transport error
- `error_model_unavailable.json` — HTTP 503 equivalent

## Capture steps

Automated via `scripts/capture-soniox-fixtures.sh`:

```bash
export SONIOX_API_KEY=sx-XXXX
bash scripts/capture-soniox-fixtures.sh path/to/sample.wav
```

The script opens a real WebSocket against `stt-rt.soniox.com` in both
source-only and two-way-translation modes, pipes the provided WAV in,
and saves the first ~40 frames to `_raw.json` arrays. You then pick the
representative single frame per fixture name (one with
`translation_status=translation`, one source-mode frame, one with the
`<end>` sentinel). Do NOT edit the field shapes — §0.5.1 exists exactly
so reality (integer `error_code` in April 2026) bites first.

After curating the frames, add each new filename to the CAPTURED table
above and add a line in `server-rs/src/features/broadcast/data/pipeline/soniox.rs`'s
`#[cfg(test)] mod tests` that `serde_json::from_str::<SonioxResponse>(…)`
the file and asserts `.is_ok()`.
