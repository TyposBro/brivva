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
| RS-003 | patched | Product/brand/offer terms improve STT/translation accuracy only if host supplies them. | Live setup now asks for optional product terms with explanation; Workers stores them on the session; server-rs forwards sanitized terms into Soniox translation context. | Validate with real product names/promo codes during next live rehearsal. | [TTS Concision: Implementation Proposal](tts_concision.md#implementation-proposal) |
| RS-004 | patched | 4K input can exceed machine/GPU/network budget if emitted as true 4K RTMP. | RTX 4060 laptop proved true 4K YouTube output connects but falls below realtime; backend caps output profile to 1080p even if env asks for 4K. | Keep 4K-host scenarios as 1080p downsample validation; revisit true 4K only with dedicated launch hardware/product need. | [Scenarios: High-Resolution Input](scenarios.md#high-resolution-input), [Scenarios: Many Outputs](scenarios.md#many-outputs) |
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
| RS-023 | patched | Translated tokens have no timestamps, so source timing must use source/original tokens while TTS budget can use available silence. | Soniox `start_ms`/`end_ms` parsing, split `source_speech_duration_ms` vs `available_window_ms`, env-gated lookahead budget, atomic timeout flush, and scheduler edge-case tests are patched. TTS logs source speech duration separately from budget and uses `tts_budget_ms()` for concision. | Live-validate `BRIVVA_TTS_LOOKAHEAD_BUDGET=1`: fast utterance + silence should show `available_window_method=NextUtteranceStart`; timeout should show `HoldTimeoutFallback`; verify no duplicate/stale dispatch in logs. | [TTS Concision](tts_concision.md), [Live Failure Policy: Translated Audio Too Long](live_failure_policy.md#translated-audio-too-long) |
| RS-024 | patched | Provider-level TTS speaking-rate control may reduce backlog better than text shortening. | ElevenLabs docs confirm `voice_settings.speed` on TTS stream/convert requests; code now emits conservative speed only for catch-up/concision/hard-recovery policies. | Live-listen JA/ZH/KO with backlog scenarios; tune `1.08/1.12/1.20` or disable if artifacts appear. | [Live Failure Policy: Translated Audio Too Long](live_failure_policy.md#translated-audio-too-long) |

## Test Automation Gaps

| ID | Status | Risk | Current Signal | Next Patch | Deep Link |
| --- | --- | --- | --- | --- | --- |
| RS-030 | patched | Bad network/backpressure behavior needs deterministic repeatable stress inputs. | MP4 smoke now supports audio delay, video/audio chunk drops, fake bad RTMP, TTS delay, and STT disable chaos flags; short Infisical e2e passed for all six scenarios on 2026-05-04. | Add JSON summaries later. | [Future Automation](future_automation.md) |
| RS-031 | patched | Full stress suite may not assert pass/fail on every bad signal yet. | Runner now fails on media drops, sustained below-realtime encode, TTS overflows, and hard recovery above configured thresholds. | Add JSON summary output for dashboards/CI. | [Signals: Useful Greps](signals.md#useful-greps) |
| RS-032 | watch | MP4 fixtures can be invalid or unsupported codec, causing misleading zero-chunk runs. | Browser path is H.264-only; fixtures should be H.264 to simulate browser ingest. | Keep fixture validation/preflight explicit before starting RTMP outputs. | [Scenarios: High-Resolution Input](scenarios.md#high-resolution-input) |
| RS-033 | watch | Accidental 4K host input must not force true 4K backend output. | 3840-wide H.264 fixture on RTX 4060 showed true 4K YouTube output is not realtime-stable; backend cap now downscales output profiles to 1080p. | Run 4K-host-to-1080p stress through normal runner and keep clear logs showing output cap. | [Scenarios: High-Resolution Input](scenarios.md#high-resolution-input) |

## Recent Patches To Watch

| Commit | Status | What Changed | Follow-Up |
| --- | --- | --- | --- |
| `c610e7b` | patched | Tightened TTS concise/hard-recovery fallback before ElevenLabs. | Rerun e2e and compare JA overflow count against 25 baseline. |
| `9a28d54` | patched | Added TTS concision strategy docs and Soniox exploration notes. | Use tracker item RS-001 before adding Soniox context code. |
| `fb24df9` | patched | Split TTS source speech timing from available-window budget; added Soniox timestamp parsing and env-gated lookahead TTS scheduler. | Rerun e2e with `BRIVVA_TTS_LOOKAHEAD_BUDGET=1` and verify by ear/logs. |
| `18e2fa7` | patched | Added lookahead scheduler race/edge tests, atomic timeout flush, and ElevenLabs speed control. | Live-listen JA/ZH/KO; tune speed values if artifacts appear. |
| `d470382` | patched | Added deterministic MP4 chaos scenarios and 4K fixture preflight; validated six short chaos e2e runs. | Run true 4K scenarios with a valid H.264 4K fixture on launch hardware. |
| `41198c3` | patched | Capped backend video output profiles to 1080p after 4K laptop proof fell below realtime. | Keep 4K-host-to-1080p stress in launch checks. |

## Standard Triage Flow

1. Start here.
2. Pick one `open` item.
3. Open only the deep-link doc for that item.
4. Inspect latest logs with [Signals](signals.md).
5. Patch narrowly.
6. Update this tracker with status, commit, and remaining risk.
