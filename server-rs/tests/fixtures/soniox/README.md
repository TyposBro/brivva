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

Capture instructions: enable `RUST_LOG=server_rs::features::broadcast::data::pipeline::soniox=trace`,
run a live session against Soniox, grep the raw WS frames from stderr, paste
into a new fixture file. Do NOT edit to "clean it up" — the point of §0.5.1
is that reality bites first.
