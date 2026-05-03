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
- H.264-only browser/RTMP direction is correct.
- Server owns FPS/resolution caps rather than trusting frontend claims.
- Original audio/video drops are observable.
- Translated TTS backlog has catch-up, whole-segment drops, and metadata logs.
- Per-language fanout has been proven under ideal local conditions.

Not yet set-and-forget:

- JA/ZH/other translated TTS expansion needs another e2e after Soniox context.
- Stress assertions now exist, but JSON summaries and CI dashboards do not.
- Bad-network and one-bad-destination chaos are not fully automated without
  external tools/platform behavior.
- 4K path needs explicit launch-tier limits and soak tests.
- Operator controls are still thin.

## Must Pass Before Paid Production

1. `backlog_catchup_many_outputs` passes with:
   - `video_stale_chunks_dropped=0`;
   - `host_audio_stale_chunks_dropped=0`;
   - `ready_host_bytes_dropped=0`;
   - no `encode below realtime`;
   - `tts segment queue overflow=0`;
   - `final_policy="hard_recovery"=0` unless explicitly allowed for a stress
     scenario.
2. One bad destination does not kill valid destinations.
3. Bad Soniox key keeps source/pass stream live.
4. Bad ElevenLabs key keeps source/pass stream live.
5. 30-60 minute soak on exact launch profile.
6. Operator can disable a target language without stopping the source stream.
7. Operator can stop/restart one output.
8. Operator can lower output profile for a stream group.

## Implemented In This Pass

- Stress runner now fails after each scenario if logs show:
  - video stale drops;
  - video keyframe-wait drops;
  - host/original audio stale drops;
  - ready host audio drops;
  - sustained below-realtime encode;
  - whole TTS segment overflow above `RUST_STRESS_MAX_TTS_OVERFLOWS`;
  - hard-recovery policy above `RUST_STRESS_MAX_HARD_RECOVERY`.
- Soniox translate sessions now include concise live-commerce context:
  - domain: `live commerce`;
  - setting: `real-time sales livestream`;
  - instructions to preserve prices/product names/stock/discounts/dates/CTA and
    avoid filler/excessive politeness.

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

## Launch Recommendation

Until all must-pass items are green, treat production as "operator-supervised":

- engineer watches logs;
- start with 1080p30;
- keep 4K disabled unless explicitly tested that day;
- have a source/pass stream as fallback;
- have a manual kill/downgrade path ready.
