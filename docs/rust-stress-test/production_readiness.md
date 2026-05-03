# Production Readiness

Goal: move `server-rs` from "pilot with Aziz watching logs" to "set it and
forget it" for paid live-commerce shows.

## Definition

Set-and-forget means:

- a non-engineer can start a show;
- bad browser/device input fails before going live;
- original host audio/video remains live under normal provider failures;
- each language/platform output can degrade independently;
- the system automatically detects bad signals and fails tests before launch;
- operators have obvious controls for stopping, restarting, or degrading one
  output without killing the whole show;
- every production incident leaves enough logs to know what failed.

## Current State

Good enough for controlled pilot:

- 1080p30 multi-output path has passed local e2e.
- Latest `backlog_catchup_many_outputs` run passed local stress assertions with
  no video drops, no host audio drops, no FFmpeg restart, no sustained
  below-realtime encode, no TTS overflow, and no hard recovery.
- H.264-only browser/RTMP direction is correct.
- Server owns FPS/resolution caps rather than trusting frontend claims.
- Original audio/video drops are observable.
- Translated TTS backlog has bounded catch-up, whole-segment drops, and
  metadata logs.
- Per-language fanout has been proven under ideal local conditions.

Not yet set-and-forget:

- Provider failures are not yet billing-safe. Soniox/ElevenLabs/RTMP failures
  are logged and partly retried, but not fully classified, sent to frontend,
  persisted as failure windows, or excluded from billing.
- JA/ZH/KO/other translated TTS needs repeated human listening checks after
  every timing/concision change.
- Stress assertions now exist, but JSON summaries and CI dashboards do not.
- Bad-network and one-bad-destination chaos are not fully automated without
  external tools/platform behavior.
- 4K path is not proven. Previous 4K attempts were blocked by fixture/codec
  issues and later focus was 1080p30. Do not claim 4K production support yet.
- AWS/Fargate production path is not proven. Local GPU success does not prove
  AWS GPU/NVENC, FFmpeg build, librtmp, fonts, secrets, network egress, log
  shipping, or container limits.
- Operator controls are still thin.

## Must Pass Before Paid Production

1. `backlog_catchup_many_outputs` passes with:
   - `video_stale_chunks_dropped=0`;
   - `host_audio_stale_chunks_dropped=0`;
   - `ready_host_bytes_dropped=0`;
   - no FFmpeg publisher crash/restart;
   - no sustained below-realtime encode beyond the configured warm-up window;
   - `tts segment queue overflow=0`;
   - `final_policy="hard_recovery"=0` unless explicitly allowed for a stress
     scenario.
   - translated audio remains intelligible in human listening checks.
2. One bad destination does not kill valid destinations.
3. Bad Soniox key keeps source/pass stream live.
4. Bad ElevenLabs key keeps source/pass stream live.
5. 30-60 minute soak on exact launch profile.
6. Operator can disable a target language without stopping the source stream.
7. Operator can stop/restart one output.
8. Operator can lower output profile for a stream group.
9. Provider failure drills pass:
   - bad Soniox key/down/rate-limit;
   - bad ElevenLabs key/quota/rate-limit;
   - bad YouTube/RTMP key;
   - one bad platform mixed with good platforms.
10. Billing excludes unbillable failure windows for affected providers,
    languages, and platform outputs.
11. AWS launch profile passes dry-run and 30-60 minute soak on the exact
    instance/container/FFmpeg build.

## Implemented In This Pass

- Stress runner now fails after each scenario if logs show:
  - video stale drops;
  - video keyframe-wait drops;
  - host/original audio stale drops;
  - ready host audio drops;
  - FFmpeg publisher crash/restart;
  - sustained below-realtime encode;
  - whole TTS segment overflow above `RUST_STRESS_MAX_TTS_OVERFLOWS`;
  - hard-recovery policy above `RUST_STRESS_MAX_HARD_RECOVERY`.
- Soniox translate sessions now include concise live-commerce context:
  - domain: `live commerce`;
  - setting: `real-time sales livestream`;
  - instructions to preserve prices/product names/stock/discounts/dates/CTA and
    avoid filler/excessive politeness.
- TTS catch-up speed is capped for intelligibility. The translated lane should
  stay near-live as continuous speech; it should not squeeze every generated
  utterance into the exact original utterance duration.

## Remaining Patches

### Operator Controls

Needed controls:

- disable one target language;
- restart one output;
- stop one platform destination;
- downgrade resolution/FPS/bitrate for one output group;
- force source-only mode.

Implementation direction:

- expose output IDs already defined by render graph / output health;
- route commands through server API or control WS;
- make commands idempotent;
- log command id, target output, before/after state.

### Stress JSON Summary

The runner should write `summary.json` per run:

```json
{
  "scenario": "backlog_catchup_many_outputs",
  "video_stale_drops": 0,
  "host_audio_stale_drops": 0,
  "ready_host_audio_drops": 0,
  "tts_overflows_by_lang": { "ja": 0 },
  "hard_recovery_by_lang": { "ja": 0 },
  "encode_below_realtime": 0,
  "result": "pass"
}
```

This lets future dashboards and CI read results without scraping prose.

### Publisher Crash Diagnostics

Latest stress finding:

- scenario: `backlog_catchup_many_outputs`;
- media backlog policy was mostly healthy;
- one translated FFmpeg/RTMP publisher exited and restarted;
- that stream then reported host-audio stale drops after restart;
- FFmpeg stderr did not include a clear platform/network reason at the crash
  point.

Needed patches:

- log per-stream buffer state when a publisher crashes and before restart;
- include exit status, last successful write age, queued video chunks, queued
  host-audio chunks/bytes, queued TTS bytes/segments, and destination platform;
- keep the stress runner failing on publisher restarts for production profiles;
- add a local RTMP sink scenario so YouTube/platform/network instability can be
  separated from Rust/FFmpeg instability.

### Product Terms

Soniox supports `terms` and `translation_terms`, but server-rs does not yet have
product/brand/promo data to pass into Soniox.

Needed data:

- product names;
- brand names;
- promo codes;
- protected phrases;
- units and product-specific claims.

These should come from session/admin metadata, not hardcoded Rust.

### Soak Tests

Required launch soak:

- 1080p30, all target languages, all launch platforms, 30-60 minutes.
- 720p15 weak-host scenario, 10 minutes.
- one bad RTMP destination, 10 minutes.
- provider failure drills: bad Soniox key and bad ElevenLabs key.
- AWS/Fargate exact launch task, all secrets, log shipping, and target
  platforms, 30-60 minutes.
- 4K only if it is part of the sold launch tier; otherwise explicitly disable
  4K in product/profile limits.

### Provider Failure And Billing

Current gap: provider failures are not yet first-class billing events. See
[Provider Failure And Billing Policy](provider_failure_billing.md).

Needed patches:

- add typed provider health events for Soniox, ElevenLabs, FFmpeg, YouTube,
  Grip, TikTok, and AWS;
- send provider/output health over WebSocket to frontend;
- persist failure windows to Workers;
- compute billable usage from healthy delivered windows;
- add circuit breakers for rate limits and upstream outages;
- add explicit frontend states for recoverable vs unrecoverable failures.

## Launch Recommendation

Until all must-pass items are green, treat production as "operator-supervised":

- engineer watches logs;
- start with 1080p30;
- keep 4K disabled unless explicitly tested that day;
- do not assume AWS behaves like the local machine until the exact AWS task has
  passed soak;
- have a source/pass stream as fallback;
- have a manual kill/downgrade path ready.
