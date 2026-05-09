# WebCodecs Production Soak, Flags, and WS Congestion Roadmap

Status: todo
Owner: Brivva engineering
Target env: Cloudflare Pages frontend + Cloudflare Workers API + AWS Rust media server + YouTube Live + Grip
Related:

- `docs/specs/todo/webcodecs-ingest-ab-spec.md`
- `docs/specs/todo/webcodecs-prod-e2e-stress-test.md`
- `docs/specs/todo/prod-e2e-automation-stress-test.md`
- `docs/launch-browser-and-media-runbook.md`
- `frontend/src/features/broadcast/presentation/use-webcodecs-video.ts`
- `server-rs/src/features/broadcast/data/session_ws/webcodecs.rs`

## Problem

The WebCodecs VP8-over-WebSocket ingest path is implemented but intentionally not promoted:

1. **Production YouTube/Grip soak has not run.** Code tests prove parsing, UI, and routing; they do not prove visible-live stability on real platforms.
2. **WebCodecs remains behind default-off flags.** This is correct until soak data shows it is safer than WebRTC for Brivva's production traffic.
3. **Audio and video currently share one media WebSocket.** This can be acceptable at 720x1280@30 and ~2.5-2.8 Mbps, but promotion requires measured proof that video backpressure does not delay STT-critical audio.

## Goal

Produce enough production evidence to decide one of:

```text
A. Keep WebRTC default; continue hardening WebRTC.
B. Promote WebCodecs WS to Auto for Chrome/Brave operator runs.
C. Keep WebCodecs experimental, split video onto /api/session/video, then retest.
D. Abandon WebCodecs WS and revisit WebTransport/QUIC later.
```

## Non-goals

- Do not make WebCodecs default before production soak passes.
- Do not remove WebRTC.
- Do not silently fallback mid-stream from WebCodecs to WebRTC after RTMP destinations are live.
- Do not store production secrets, OAuth tokens, RTMP keys, or Grip credentials in artifacts.
- Do not claim Grip success from server-side FFmpeg logs alone; require platform-visible live evidence.
- Do not claim audio safety from video-only tests; STT/TTS/audio timing must be measured under video load.

## Feature flags and rollout controls

### Current flags

Frontend:

```text
VITE_WEBCODECS_INGEST_ENABLED=false by default
```

Server:

```text
BRIVVA_WEBCODECS_INGEST_ENABLED=false by default
```

Both must be enabled for WebCodecs to be selectable and usable.

### Rollout rule

| Stage | Frontend flag | Server flag | Auto resolves to | Allowed users | Purpose |
| --- | --- | --- | --- | --- | --- |
| 0 | off | off | WebRTC | all | current safe prod |
| 1 | on | on | WebRTC | Aziz/operator only | explicit WebCodecs A/B |
| 2 | on | on | WebRTC | selected internal sessions | soak + congestion proof |
| 3 | on | on | WebCodecs for Chrome/Brave | operator-only | canary default |
| 4 | on | on | WebCodecs where proven | broader | product default candidate |

### Kill switch

Rollback is instant config-only:

```text
VITE_WEBCODECS_INGEST_ENABLED=false
BRIVVA_WEBCODECS_INGEST_ENABLED=false
```

Expected rollback behavior:

- setup UI disables WebCodecs;
- server capabilities omit `webcodecs_ws`;
- `Auto` stays WebRTC;
- explicit WebCodecs attempts fail before recording/RTMP publish.

## Required credentials and production access

### YouTube

Need:

- production Google OAuth connection for `E2E_USER_ID`;
- YouTube quota headroom for broadcast creation, stream binding, health polling, and VOD metadata;
- permission to create unlisted test broadcasts;
- watcher access to YouTube watch/VOD pages.

### Grip

Need one of:

1. official Grip Seller API credentials that can create/read live broadcasts and stream ingest state; or
2. operator-provided Grip RTMP destination + watch URL / seller-center evidence path.

Grip proof must include at least one of:

- public/host-visible live page screenshot/video;
- Seller Center live-state evidence;
- API-confirmed live/ended/VOD artifact;
- post-run recording/VOD duration if Grip provides it.

## Test matrix

### Smoke matrix — 10 minutes each

| Browser | Mode | Platform | Shape | Required? |
| --- | --- | --- | --- | --- |
| Brave/Chromium | WebRTC | YouTube | translated | yes |
| Brave/Chromium | WebCodecs WS | YouTube | translated | yes |
| Brave/Chromium | WebRTC | Grip | source/pass | yes |
| Brave/Chromium | WebCodecs WS | Grip | source/pass | yes |
| Brave/Chromium | WebRTC | YouTube + Grip | dual or translated+source | yes if destinations available |
| Brave/Chromium | WebCodecs WS | YouTube + Grip | dual or translated+source | yes if destinations available |
| Firefox/Zen | WebRTC | YouTube | translated | compatibility evidence |
| Firefox/Zen | WebCodecs WS | YouTube | translated | only if APIs + visible-live work |

### Soak matrix — 30-60 minutes each

Run only after smoke passes:

| Browser | Mode | Platform | Shape |
| --- | --- | --- | --- |
| Brave | WebRTC | YouTube | translated |
| Brave | WebCodecs WS | YouTube | translated |
| Brave | WebRTC | Grip | source/pass or translated if available |
| Brave | WebCodecs WS | Grip | source/pass or translated if available |
| Brave | WebRTC | YouTube + Grip | dual-output stress |
| Brave | WebCodecs WS | YouTube + Grip | dual-output stress |

## Audio/video shared WebSocket congestion tests

### Risk hypothesis

WebCodecs video chunks can increase `WebSocket.bufferedAmount`. If the same socket also carries audio PCM, browser or network backpressure could delay audio frames, causing:

- STT latency spikes;
- Soniox stale/idle behavior;
- TTS delay/drift growth;
- host audio stale drops in FFmpeg;
- bad billing/customer experience even if video looks stable.

### Required metrics

Frontend session logs must capture:

- `frontend.webcodecs_stats`:
  - sent frames;
  - dropped frames;
  - queue ms;
  - WS buffered bytes;
  - encoder queue size;
  - server accepted frames;
  - server latest media PTS.
- `frontend.webcodecs_video_drop`:
  - reason;
  - dropped frame count;
  - buffered amount;
  - queue ms.
- audio bridge health:
  - audio chunk send cadence if available;
  - AudioWorklet/AudioPipeline errors;
  - timestamped audio sample timeline if `VITE_BRIVVA_V2_TIMESTAMPED_AUDIO` is enabled.

Server/CloudWatch/session logs must capture:

- `server.webcodecs_video_start`;
- `server.webcodecs_video_first_keyframe`;
- `server.webcodecs_video_frame_gap`;
- `server.webcodecs_stats`;
- `server.video_ingest_stopped`;
- host audio stale/drop counters;
- TTS delay/drift/overflow;
- STT reconnect/error logs;
- FFmpeg speed/restart/drop logs.

### Congestion drills

Run each drill against WebRTC and WebCodecs so deltas are comparable.

| Drill | Method | Expected WebCodecs behavior | Fail trigger |
| --- | --- | --- | --- |
| Normal uplink | no throttling | WS buffered p95 < 4 MB, queue p95 < 500 ms | video drops or audio delay grows |
| Moderate uplink throttle | browser/CDP/network shaping or Linux `tc` | drops old non-keyframes, requests keyframe, audio OK | STT/TTS p95 worse than WebRTC by >10% |
| Severe uplink throttle | lower bandwidth near video bitrate | video quality/drops degrade first | audio chunks delayed/dropped |
| CPU pressure | background CPU load on host | encoder queue may rise, drops logged | browser freeze, audio late |
| Long run | 30-60 min | no unbounded queue, stable FFmpeg speed | memory/queue growth, restarts |

### Split-WS decision rule

Create `/api/session/video` and move video off the audio/control socket if any production-like WebCodecs run shows:

- audio STT final latency p95 > WebRTC baseline by 10% or > 750 ms absolute regression;
- TTS delay p95 > WebRTC baseline by 10% or > configured threshold;
- `host_audio_stale_chunks_dropped > 0` during normal network conditions;
- `WebSocket.bufferedAmount` p95 > 4 MB for > 60 seconds and audio timing degrades;
- audio sender cadence gaps correlate with WebCodecs video queue spikes;
- Soniox reconnects/idle timeouts correlate with video backpressure.

## Promotion gates

### Setup/live gates

- Explicit WebCodecs selection is visible on `/session/:id/setup`.
- Live diagnostics show active resolved mode.
- Server capabilities include `webcodecs_ws` only when server flag is enabled.
- If frontend/server/browser support is missing, Record is blocked before RTMP publish.

### YouTube gates

- YouTube provider-confirmed live ratio >= 0.90 for 10m smoke, >= 0.95 for 30-60m soak.
- Watch page/VOD evidence exists.
- FFmpeg restarts = 0 during normal soak.
- FFmpeg speed min-after-warmup >= 0.98.
- Visible/VOD video cadence has no sustained stalls.
- Source/translated audio is audible and not drifting beyond threshold.

### Grip gates

- Grip visible-live/API-confirmed live for >= 90% of run.
- No Grip-specific FFmpeg/librtmp failures.
- If Grip VOD exists, duration matches run duration within tolerance.
- If Grip has no reliable API/VOD, operator-visible screenshots/video are mandatory artifacts.

### WebCodecs gates

- `server.webcodecs_video_first_keyframe` observed within 3 seconds of start.
- Server accepted frames > 0 and roughly matches client sent frames after expected drops.
- Sequence gaps are explained by frontend drop logs, not silent loss.
- Normal run `frontend.webcodecs_video_drop` count = 0 or near-zero; throttled runs may drop video but not audio.
- WS buffered p95 < 4 MB during normal runs.
- WebCodecs does not increase TTS delay p95/drift versus WebRTC beyond 10%.

## Execution phases

### Phase 1 — Flagged production smoke

1. Deploy frontend/server with flags still default-off.
2. Enable both flags only in the target prod environment/operator window.
3. Run YouTube 10m WebRTC baseline.
4. Run YouTube 10m WebCodecs.
5. Run Grip 10m WebRTC baseline.
6. Run Grip 10m WebCodecs.
7. Compare artifacts.

### Phase 2 — Dual-destination stress

Run one session publishing to YouTube + Grip from the same host ingest.

Purpose:

- catch per-platform FFmpeg/RTMP divergence;
- validate shared encoded input with multiple drains;
- verify WebCodecs does not solve YouTube while breaking Grip or vice versa.

### Phase 3 — Shared-WS congestion proof

Run normal and throttled WebCodecs sessions with timestamped audio enabled.

Decision:

- if audio is clean, continue WebCodecs WS soak;
- if audio degrades, implement split video WS before any default promotion.

### Phase 4 — 30-60 min soak

Run finalists:

```text
Brave WebRTC YouTube 30-60m
Brave WebCodecs WS YouTube 30-60m
Brave WebRTC Grip 30-60m
Brave WebCodecs WS Grip 30-60m
Brave WebRTC YouTube+Grip 30-60m
Brave WebCodecs WS YouTube+Grip 30-60m
```

### Phase 5 — Decision record

Write a dated decision doc/artifact containing:

- runs included;
- summary table;
- failed gates;
- known caveats;
- chosen default/follow-up path;
- rollback instructions.

## Rollback plan

If WebCodecs fails during smoke/soak:

1. Stop active broadcast from UI.
2. Disable frontend/server flags.
3. Verify setup UI disables WebCodecs.
4. Create a fresh WebRTC session; do not reuse a half-started WebCodecs live session.
5. Mark artifacts as failed and keep logs.
6. If RTMP platform broadcasts remain active, end/delete them from platform/API.

## Artifacts

Every production run must preserve:

```text
meta.json
created.redacted.json
watch-urls.json
host-console.log
host-ws.ndjson
session-logs.ndjson
provider-health.ndjson
cloudwatch-filtered.ndjson
cf-worker-tail.ndjson or cf-observability-summary.json
screenshots/
host-video.webm
watcher-video-*.webm
youtube-vod-metadata.json when available
grip-live-evidence.* when available
summary.json
verdict.md
```

## Done when

- At least one 10m YouTube smoke exists for WebRTC and WebCodecs.
- At least one 10m Grip smoke exists for WebRTC and WebCodecs.
- At least one 30-60m YouTube soak exists for both finalist modes.
- At least one 30-60m Grip soak exists for both finalist modes, or a documented Grip blocker exists.
- Shared-WS congestion measurements show audio is safe, or split-WS is specified as required before promotion.
- Decision is recorded: keep WebRTC default, promote WebCodecs for a scoped browser/operator set, or build split video WS first.
