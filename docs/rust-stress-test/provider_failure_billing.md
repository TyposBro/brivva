# Provider Failure And Billing Policy

Goal: paid sessions should charge only for service that was actually delivered.
If Soniox, ElevenLabs, YouTube, Grip, TikTok, AWS, or FFmpeg fails in a way the
viewer cannot use, the frontend and billing layer must know which part failed
and whether that time is billable.

## Current State

Implemented:

- FFmpeg/RTMP outputs have internal health states:
  `starting`, `live`, `degraded`, `restarting`, `failed`, `stopped`.
- FFmpeg publisher crashes can be detected and restarted.
- RTMP publish errors and restart-limit failures are logged.
- Soniox websocket connect/read failures reconnect.
- Soniox `error_code` responses are parsed and logged.
- ElevenLabs non-2xx, request timeout, request error, and stream chunk errors
  are logged.
- Soniox error responses now emit `provider_health` websocket messages to the
  frontend with `billable=false`.
- ElevenLabs TTS failures now emit `provider_health` websocket messages to the
  frontend with `billable=false`.
- Frontend accepts `provider_health` messages and surfaces unbillable provider
  problems as connection issues.
- TTS failure does not crash the whole source/pass stream.
- Workers accepts session metrics through
  `PATCH /internal/sessions/:id/metrics`.
- Output billing tracks delivered TTS PCM seconds per language.
- Workers persists provider failure windows from server-rs and usage/summary
  subtract unbillable source/lang windows from billable minutes while exposing
  provider/lang audit buckets.

Not implemented enough for set-and-forget production:

- Soniox/ElevenLabs/RTMP health now reaches frontend and Workers failure-window
  persistence, but platform-specific visible-live proof is still incomplete.
- ElevenLabs has a language-scoped circuit breaker; Soniox/RTMP backoff exists,
  but broader provider circuit-breaker policy still needs launch tuning.
- Per-platform invoice policy still needs final product mapping once platform
  SKUs exist.
- YouTube RTMP acceptance is not the same as a visible/started YouTube Live
  event. Without YouTube Live API integration, the server can prove bytes were
  pushed but cannot prove Studio published the event.
- AWS/Fargate production behavior has not been proven. Local GPU success does
  not prove AWS instance GPU, network, FFmpeg build, librtmp, fonts, IAM,
  secrets, disk, or container limits.

## Failure Classes

Use one shared failure model for all providers:

```json
{
  "type": "provider_health",
  "sessionId": "session-id",
  "outputId": "youtube-ja",
  "provider": "elevenlabs",
  "state": "degraded",
  "recoverable": true,
  "billable": false,
  "reason": "rate_limited",
  "statusCode": 429,
  "message": "ElevenLabs rate limited Japanese TTS; captions/source continue",
  "startedAt": "2026-05-03T10:00:00Z"
}
```

Recommended fields:

- `provider`: `soniox | elevenlabs | youtube | grip | tiktok | ffmpeg | aws`.
- `scope`: `session | output | lang | platform`.
- `state`: `live | degraded | reconnecting | failed | recovered`.
- `recoverable`: whether automatic retry/backoff may recover it.
- `billable`: whether this provider/output time should count toward billing.
- `reason`: stable machine-readable reason.
- `statusCode` or `errorCode`: upstream status when available.
- `outputId`, `lang`, `platform`: identify affected lane.
- `startedAt`, `recoveredAt`: required for billing windows.

## Provider-Specific Policy

### Soniox

Recoverable:

- websocket close;
- network timeout;
- `408` timeout;
- transient `5xx`.

Behavior:

- reconnect with bounded exponential backoff;
- keep source/pass stream live;
- mark translation lanes `reconnecting`;
- keep billing for source/pass only;
- stop translation/STT billing while Soniox is unavailable.

Unrecoverable until operator/config fix:

- `401` or `403` auth/key failure;
- repeated `429` rate limit beyond retry budget;
- invalid model/language/config;
- reconnect budget exhausted.

Frontend:

- show target languages as degraded/unavailable;
- show "source stream still live";
- show "translation not billable while provider unavailable."

### ElevenLabs

Recoverable:

- network timeout;
- transient `5xx`;
- stream chunk error for one utterance.

Behavior:

- retry with backoff for the utterance if still near live edge;
- otherwise skip the utterance and keep stream live;
- optionally degrade to captions-only for affected language;
- do not block source/pass stream.

Unrecoverable until operator/config/account fix:

- `401` or `403` auth/key failure;
- `402` quota/payment;
- sustained `429` rate limit;
- unsupported voice/model.

Frontend:

- show affected language audio as degraded/unavailable;
- keep subtitles/translation if Soniox still works;
- mark translated audio unbillable while TTS is unavailable.

### YouTube / Grip / TikTok / RTMP Platforms

Recoverable:

- transient broken pipe;
- short network stall;
- FFmpeg child crash below restart limit.

Behavior:

- restart only the failed output;
- keep other platforms/languages live;
- mark affected output `restarting`;
- continue billing only for outputs that remain live.

Unrecoverable until operator/platform fix:

- bad stream key;
- RTMP publish rejected repeatedly;
- platform quota/event disabled;
- restart limit reached;
- platform accepts RTMP bytes but no live event is bound/started.

Frontend:

- show exact output label and key env name to monitor;
- show failed output as not billable after failure time;
- do not hide successful sibling outputs.

### AWS / Fargate / GPU Host

Recoverable:

- one task crash with replacement task starting cleanly;
- temporary metrics PATCH failure to Workers;
- transient network loss below live lag window.

Unrecoverable/session-impacting:

- no NVENC/GPU available for selected profile;
- FFmpeg build missing `librtmp`, `libharfbuzz`, `drawtext`, or NVENC;
- secrets unavailable;
- container CPU/memory/GPU throttling;
- network egress too slow for configured fanout;
- font package missing for subtitles;
- health checks fail or task cannot restart outputs.

Policy:

- fail fast before show if GPU/FFmpeg capabilities do not match requested
  profile;
- downgrade to source/pass or lower profile only with visible frontend state;
- mark affected output/session windows unbillable if viewers could not receive
  usable stream.

## Billing Rules

Recommended billing units:

- Source/pass stream: bill only while source output is healthy and delivering
  bytes/frames to at least one requested destination.
- Translated subtitles/STT: bill only while Soniox translation lane is healthy.
- Translated audio/TTS: bill only for delivered TTS audio seconds that were
  actually sent to an active output.
- Platform fanout: bill only while the destination output is live or within a
  short recoverable reconnect grace window.

Do not bill:

- provider auth failure;
- quota/payment failure;
- sustained rate limit;
- output after restart limit reached;
- language lane while Soniox/ElevenLabs is down;
- YouTube/Grip/TikTok output that never became a visible live event if platform
  integration can prove that state.

Billing implementation direction:

1. Add provider/output health events with `billable=false`. ✅
2. Persist failure windows to Workers. ✅
3. Compute invoice usage from healthy delivered windows, not only expected
   session duration. ✅ for source/lang metrics; platform SKU mapping later.
4. Keep current TTS PCM seconds as a useful lower-level metric, but do not rely
   on it alone for full session billing.

## Frontend Contract

Frontend needs more than a generic error toast. It should show:

- Source stream: live/degraded/failed.
- Each language: STT live, TTS live, captions live.
- Each platform output: publishing/restarting/failed.
- Whether the failure is billable.
- Whether the operator can retry, downgrade, or disable only that output.

Example user-facing messages:

- "Japanese audio is temporarily unavailable because ElevenLabs is rate
  limiting. Japanese captions and original stream continue. This period is not
  billable for Japanese audio."
- "YouTube JA output failed after repeated publish errors. Other outputs remain
  live. This output is not billable after 10:03:12."
- "Your browser/device cannot start H.264 live stream."
- "GPU encoder unavailable on this AWS task. Start blocked for 4K profile."

## Test Matrix

Required before claiming production-ready:

- Bad Soniox key: source/pass stays live; translation lanes fail visibly; no
  translated billing.
- Soniox down/timeout: reconnects; frontend shows reconnecting; billing pauses
  affected lanes.
- Soniox rate limit: backs off; marks affected lanes unbillable.
- Bad ElevenLabs key: captions/source continue; TTS lane fails visibly; no TTS
  billing.
- ElevenLabs `429`: backs off/circuit-breaks; no unreadable fast audio; no TTS
  billing during outage.
- Bad YouTube key: that output fails; sibling outputs remain live; output not
  billable.
- One bad RTMP destination mixed with good destinations: only bad output fails.
- YouTube Studio event not started/bound: documented as platform-state unknown
  unless YouTube Live API validation is enabled.
- AWS dry run: verify FFmpeg build, NVENC, librtmp, fonts, secrets, network
  egress, Workers metrics, and log shipping before any paid show.
- AWS soak: exact launch profile, 30-60 minutes, with all target platforms and
  languages.

## Current Verdict

Local 1080p30 multi-output is promising. It is not yet billing-safe
set-and-forget production because provider failures are not fully typed,
frontended, persisted, and excluded from billing.

4K is not proven. AWS is not proven. Do not sell or promise 4K/AWS production
reliability until the test matrix above passes on the exact deployment profile.
