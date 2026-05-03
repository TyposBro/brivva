# Rust Streaming Tracker

Purpose: one compact file for "what might be broken?" and "what patch is next?"
Use the deeper docs for context only after choosing an item here.

Status values:

- `open`: likely needs code or product validation.
- `watch`: currently acceptable, keep monitoring in e2e.
- `patched`: code landed; needs regression coverage or periodic e2e.
- `blocked`: needs vendor/platform confirmation or product data.

## High Priority

| ID | Status | Risk | Current Signal | Next Patch | Deep Link |
| --- | --- | --- | --- | --- | --- |
| RS-001 | watch | JA translated TTS can exceed live window and drop whole segments. | Last full e2e before stronger concision had 25 `tts segment queue overflow`, all `lang=ja`. | Soniox translate context is patched; rerun `backlog_catchup_many_outputs` and compare overflow/hard recovery counts. | [TTS Concision](tts_concision.md), [Live Failure Policy: Translated Audio Too Long](live_failure_policy.md#translated-audio-too-long) |
| RS-002 | watch | Current concision fallback is deterministic and hand-authored; it should remain safety net, not main quality layer. | Code now removes filler and preserves commerce clauses, but phrase lists are not infinitely scalable. | Soniox context now requests concise style upstream; later add feature-flag rewrite provider only if needed. | [TTS Concision: Better Than Hand Rules](tts_concision.md#better-than-hand-rules) |
| RS-003 | open | Product/brand/offer terms are not yet fed into Soniox context. | `translation_terms` documented by Soniox, but server config does not populate them from session metadata. | Extend session/admin model to carry product terms; serialize into Soniox `terms`/`translation_terms`. | [TTS Concision: Implementation Proposal](tts_concision.md#implementation-proposal) |
| RS-004 | watch | 4K and many-output paths can exceed machine/GPU/network budget. | 1080p30 many-output passed; 4K path was previously not fully proven under all variants. | Keep scenarios split: 1080p many-output, 4K capped-to-1080, true 4K where GPU budget allows. | [Scenarios: High-Resolution Input](scenarios.md#high-resolution-input), [Scenarios: Many Outputs](scenarios.md#many-outputs) |
| RS-005 | watch | A translated FFmpeg/RTMP publisher can crash/restart under many-output stress. | Later 2026-05-03 run passed with no FFmpeg restart, but this remains a production assertion. | Keep production stress failing on restart; add local RTMP sink scenario. | [Production Readiness: Publisher Crash Diagnostics](production_readiness.md#publisher-crash-diagnostics) |
| RS-006 | open | Provider failures are not billing-safe yet. | Soniox/ElevenLabs now emit frontend `provider_health` with `billable=false`; RTMP/platform health and Workers failure-window persistence are still missing. | Add Workers failure windows, RTMP/platform provider health, circuit breakers, and billing exclusion. | [Provider Failure And Billing](provider_failure_billing.md) |
| RS-007 | open | AWS production path is not proven. | Local 1080p30 passed; AWS/Fargate GPU/FFmpeg/librtmp/fonts/secrets/network behavior has not passed soak. | Add AWS dry-run checklist and exact launch-profile soak before paid production. | [Provider Failure And Billing: AWS](provider_failure_billing.md#aws--fargate--gpu-host), [Production Readiness](production_readiness.md#soak-tests) |

## Media Pipeline

| ID | Status | Risk | Current Signal | Next Patch | Deep Link |
| --- | --- | --- | --- | --- | --- |
| RS-010 | patched | Frontend/browser could lie about FPS. | Server now derives FPS/timing from received media path instead of trusting frontend caps. | Keep e2e coverage for 720p15, 1080p30/60, 4K30. | [Scenarios: Low-Quality Host](scenarios.md#low-quality-host), [Scenarios: High-Resolution Input](scenarios.md#high-resolution-input) |
| RS-011 | patched | Frontend/browser could lie about H.264 dimensions. | Server parses H.264 SPS for resolution and clamps output profile. | Keep SPS parsing unit tests and MP4 fixture tests. | [Signals](signals.md), [Scenarios](scenarios.md) |
| RS-012 | patched | Unsupported browser codec would break RTMP platform ingest. | Server/browser path is intended H.264-only; unsupported devices should fail early. | Verify frontend UI shows clear "H.264 unavailable" error. | [Live Failure Policy: Browser Cannot Provide H.264](live_failure_policy.md#browser-cannot-provide-h264) |
| RS-013 | watch | FFmpeg/RTMP backpressure can create stale A/V. | Logs now expose video/audio stale drops and catch-up state. | Add automated bad network flags so this does not require manual `tc`. | [Future Automation](future_automation.md), [Scenarios: Bad Network](scenarios.md#bad-network) |
| RS-014 | open | One FFmpeg encode feeding multiple RTMP publishers can fail differently by platform/library. | Per-language fanout works under ideal test; tee/librtmp quirks still need production soak. | Keep per-process publish fallback; add one-bad-destination automation. | [Scenarios: One Bad Destination](scenarios.md#one-bad-destination) |
| RS-015 | watch | Grip integration is not proven by current YouTube-only stress runs. | 2026-05-03 `grip_smoke` reached Grip with `720x1280`, H.264, 1s keyframes, <3 Mbps, and increasing chunks. All RTMP outputs now default to the same mobile portrait profile. | Keep rerunning `grip_smoke` with fresh one-shot creds before production events. | [Scenarios: Grip Smoke](scenarios.md#grip-smoke), [Scenarios: Mobile-First RTMP Output](scenarios.md#mobile-first-rtmp-output) |

## Translation And TTS

| ID | Status | Risk | Current Signal | Next Patch | Deep Link |
| --- | --- | --- | --- | --- | --- |
| RS-020 | patched | Long host rambles can become one giant TTS request. | Response processor flushes on endpoint, punctuation, three sentences, and length. | Tune only from logs; avoid tiny chunks that overload ElevenLabs. | [Live Failure Policy: Long Ramble / No Pause](live_failure_policy.md#long-ramble--no-pause) |
| RS-021 | patched | TTS backlog may grow briefly even when media pipeline is healthy. | TTS playback speed rises during catch-up and should return to `1.00`; max catch-up is now bounded for intelligibility. | Watch `tts_playback_speed`, `tts_buffered_bytes`, and human listening quality in every e2e. | [Signals](signals.md), [Live Failure Policy: Slight TTS Backlog](live_failure_policy.md#slight-tts-backlog) |
| RS-022 | patched | Severe TTS backlog should not cut words mid-audio. | Queue drops whole `TtsSegment`s at hard cap. | Keep whole-segment metadata logs stable for debugging. | [Live Failure Policy: Severe TTS Backlog](live_failure_policy.md#severe-tts-backlog) |
| RS-023 | open | Translated tokens have no timestamps, so source-duration estimates are imperfect. | Soniox docs say original tokens have timestamps; translation tokens do not. | Carry source/original token timing into `TtsRequest` instead of estimating from translated text chars. | [TTS Concision: Soniox Findings](tts_concision.md#soniox-findings) |
| RS-024 | blocked | Provider-level TTS speaking-rate control may reduce backlog better than text shortening. | Not confirmed for current ElevenLabs model/voice path. | Check ElevenLabs docs before adding request fields. | [Live Failure Policy: Translated Audio Too Long](live_failure_policy.md#translated-audio-too-long) |

## Test Automation Gaps

| ID | Status | Risk | Current Signal | Next Patch | Deep Link |
| --- | --- | --- | --- | --- | --- |
| RS-030 | open | Bad network/backpressure tests still require OS `tc` or live platform behavior. | `future_automation.md` lists missing chaos flags. | Add smoke flags for audio delay, video drop, audio drop, bad RTMP, TTS delay, STT disable. | [Future Automation](future_automation.md) |
| RS-031 | patched | Full stress suite may not assert pass/fail on every bad signal yet. | Runner now fails on media drops, sustained below-realtime encode, TTS overflows, and hard recovery above configured thresholds. | Add JSON summary output for dashboards/CI. | [Signals: Useful Greps](signals.md#useful-greps) |
| RS-032 | watch | MP4 fixtures can be invalid or unsupported codec, causing misleading zero-chunk runs. | Browser path is H.264-only; fixtures should be H.264 to simulate browser ingest. | Keep fixture validation/preflight explicit before starting RTMP outputs. | [Scenarios: High-Resolution Input](scenarios.md#high-resolution-input) |
| RS-033 | open | 4K is not proven end-to-end. | 4K fixture run produced zero chunks due fixture/codec issue; no clean 4K AWS/local pass exists yet. | Convert fixture to H.264 browser-ingest simulation, preflight codec, then run true 4K and capped 1080p profiles. | [Scenarios: High-Resolution Input](scenarios.md#high-resolution-input) |

## Recent Patches To Watch

| Commit | Status | What Changed | Follow-Up |
| --- | --- | --- | --- |
| `c610e7b` | patched | Tightened TTS concise/hard-recovery fallback before ElevenLabs. | Rerun e2e and compare JA overflow count against 25 baseline. |
| `9a28d54` | patched | Added TTS concision strategy docs and Soniox exploration notes. | Use tracker item RS-001 before adding Soniox context code. |
| pending | patched | Bounded TTS catch-up speed after Japanese became unintelligible at `3.0x`; expanded backlog windows and stress assertions. | Rerun many-output e2e and verify by ear, not only by logs. |

## Standard Triage Flow

1. Start here.
2. Pick one `open` item.
3. Open only the deep-link doc for that item.
4. Inspect latest logs with [Signals](signals.md).
5. Patch narrowly.
6. Update this tracker with status, commit, and remaining risk.
