# WebCodecs Ingest Production E2E A/B Stress Test Plan

Status: todo
Owner: Brivva engineering
Target env: Cloudflare Pages frontend + Cloudflare Workers API + AWS Rust media server + YouTube Live + Grip
Related:

- `docs/specs/todo/webcodecs-ingest-ab-spec.md`
- `docs/specs/todo/webcodecs-production-soak-roadmap.md`
- `docs/specs/todo/prod-e2e-automation-stress-test.md`
- `scripts/prod-media-stress-e2e.mjs`
- `scripts/analyze-prod-media-stress.mjs`

## Goal

Extend the production media stress harness into a repeatable A/B runner for the new ingest modes:

```text
WebRTC hardened
WebCodecs over WebSocket experimental
```

The runner must prove, with production artifacts, whether WebCodecs VP8-over-WebSocket is more deterministic than WebRTC without making audio/STT/TTS worse.

## Non-goals

- Do not replace the general prod media stress harness; reuse/extend it.
- Do not run fake local RTMP sinks as final proof.
- Do not claim WebCodecs success unless the server accepted WebCodecs frames and the platform was visible-live/API-live.
- Do not infer Grip success from YouTube results.
- Do not leak OAuth tokens, RTMP keys, or Grip credentials.
- Do not promote WebCodecs automatically after one green smoke; this plan produces evidence for the rollout decision.

## Existing automation baseline

Current entrypoints already exist:

```bash
bun run test:e2e:prod-media-stress
bun run analyze:e2e:prod-media-stress -- <run-dir>
bun run compare:e2e:media-ingest -- <out-dir> <run-dir...>
```

Current mode override:

```bash
E2E_MEDIA_INGEST_MODE=auto|webrtc|webcodecs_ws
```

Current artifacts include:

```text
meta.json
runner-events.ndjson
host-console.log
host-ws.ndjson
session-logs.ndjson
provider-health.ndjson
session-summary.json
cloudwatch-filtered.ndjson
cf-worker-tail.ndjson
youtube-vod-metadata.json
summary.json
verdict.md
```

This spec defines the missing WebCodecs-specific orchestration, gates, platform matrix, and comparison output.

## Required environment

### Common

```bash
BRIVVA_PROD_FRONTEND_URL=https://brivva.pages.dev
BRIVVA_PROD_API_URL=https://brivva-api.milliytechnology.workers.dev
E2E_USER_ID=111778147327359525059
E2E_MEDIA_FILE=$HOME/Desktop/text.mp4
E2E_RECORD_SECONDS=600
E2E_BROWSER=brave|chromium|firefox|zen
E2E_HEADLESS=true|false
E2E_TEST_SHAPE=source|translated|dual
E2E_MEDIA_INGEST_MODE=webrtc|webcodecs_ws
E2E_FETCH_CLOUDWATCH=true
E2E_FETCH_CF_OBSERVABILITY=true
```

### WebCodecs production flags

The deployed frontend/server must have both enabled for WebCodecs runs:

```text
VITE_WEBCODECS_INGEST_ENABLED=true
BRIVVA_WEBCODECS_INGEST_ENABLED=true
```

The runner must fail fast if explicit `E2E_MEDIA_INGEST_MODE=webcodecs_ws` but:

- setup UI disables WebCodecs;
- `server:capabilities` omits `webcodecs_ws`;
- browser APIs are missing;
- live diagnostics do not show WebCodecs as active.

### Platform selection

Existing runner defaults to YouTube. Add/standardize:

```bash
E2E_PLATFORM_MATRIX=youtube|grip|youtube,grip
E2E_GRIP_WATCH_URL=<optional public/seller-center URL>
E2E_GRIP_RTMP_URL=<provided securely, never printed>
E2E_GRIP_STREAM_KEY=<provided securely, never printed>
```

If Grip streams are created through Workers/Seller API later, replace direct RTMP envs with API-driven session creation and preserve the same artifact schema.

## Runner roadmap

### Phase 1 — Single-run mode assertions

Enhance `scripts/prod-media-stress-e2e.mjs` to verify requested mode before Record:

1. Set `localStorage.brivva:mediaIngestMode` before `/session/:id/setup` loads.
2. Capture setup body text and assert selected mode is visible.
3. If mode is WebCodecs, assert option is enabled.
4. On live page, capture diagnostics after Record.
5. Assert live diagnostics include:
   - `Media ingest: WebCodecs over WebSocket` for WebCodecs;
   - `Media ingest: WebRTC` for WebRTC.
6. Record `requested_media_ingest_mode` and `resolved_media_ingest_mode` in `meta.json`.

### Phase 2 — WebSocket/control-frame capture

`host-ws.ndjson` should record enough to prove the media path:

WebRTC run must show:

```json
{ "type": "webrtc:offer" }
{ "type": "webrtc:answer" }
```

WebCodecs run must show:

```json
{ "type": "server:capabilities", "videoIngestModes": ["webrtc", "webcodecs_ws"] }
{ "type": "video:webcodecs_start", "codec": "vp8" }
{ "type": "video:webcodecs_ready", "accepted": true }
{ "type": "video:webcodecs_stats", "frames_received": 1 }
```

Binary video frames are not logged raw. Log only counters/metadata from session logs and browser stats.

### Phase 3 — Analyzer WebCodecs gates

Extend `scripts/analyze-prod-media-stress.mjs` gates for WebCodecs runs:

| Gate | Pass condition |
| --- | --- |
| `media_ingest_mode_matches_request` | explicit requested mode equals live active mode |
| `webcodecs_capability_advertised` | server capabilities include `webcodecs_ws` and `vp8` |
| `webcodecs_start_ready` | ready frame observed before/near Record start |
| `webcodecs_server_first_keyframe` | server logs first keyframe |
| `webcodecs_frames_reached_server` | server accepted frames > 0 |
| `webcodecs_frame_gap_rate_ok` | unexplained sequence gaps <= threshold |
| `webcodecs_drop_rate_ok` | normal run dropped frames <= threshold |
| `webcodecs_ws_buffered_ok` | p95 bufferedAmount < 4 MB in normal run |
| `webcodecs_audio_not_degraded` | audio/STT/TTS metrics no worse than WebRTC paired baseline |

Keep existing gates:

- provider confirmed live ratio;
- FFmpeg speed/restart/drop;
- CloudWatch export success;
- Cloudflare worker errors;
- TTS completion/delay/drift;
- watch/VOD evidence.

### Phase 4 — Paired A/B orchestration

Add a wrapper script or mode:

```bash
node scripts/prod-media-ingest-ab-e2e.mjs
```

or extend current runner with:

```bash
E2E_MEDIA_INGEST_MATRIX=webrtc,webcodecs_ws
E2E_BROWSER_MATRIX=brave
E2E_PLATFORM_MATRIX=youtube,grip
E2E_TEST_SHAPE=translated
E2E_RECORD_SECONDS=600
bun run test:e2e:prod-media-stress
```

Wrapper responsibilities:

1. Generate one parent run id.
2. Run WebRTC baseline first.
3. Run WebCodecs with identical:
   - browser;
   - fixture;
   - source/target langs;
   - platform shape;
   - duration;
   - CloudWatch/CF capture settings.
4. Leave enough cooldown between platform broadcasts to avoid API/processing overlap.
5. Write parent comparison:

```text
tmp/prod-media-ingest-ab-runs/<parent-run-id>/
  webrtc/<normal prod-media-stress artifacts>
  webcodecs_ws/<normal prod-media-stress artifacts>
  summary.json
  verdict.md
```

### Phase 5 — Grip automation

Add Grip support in layers.

#### Grip Level 1 — manual RTMP + evidence

Runner accepts secure RTMP URL/key and optional watch/evidence URL.

Artifacts:

```text
grip-watch-before.png
grip-watch-end.png
grip-body-final.txt
grip-operator-notes.md
grip-live-evidence.webm optional
```

Pass requires operator/API evidence, not just FFmpeg logs.

#### Grip Level 2 — API polling

If official Seller API access is available, poll:

- broadcast/live state;
- ingest health if exposed;
- viewer/VOD/recording state if exposed.

Write:

```text
grip-provider-health.ndjson
grip-vod-metadata.json
```

Normalize into `streams[]` summary fields like YouTube.

#### Grip Level 3 — full watcher automation

If a public/seller-center live page can be automated safely:

- open watcher before Record;
- take screenshots every 30s;
- record page video;
- reload after Stop;
- collect page text/status.

## Test shapes

### Shape 1 — WebCodecs source/pass baseline

Purpose: isolate video ingest and original audio without translation load.

```bash
E2E_TEST_SHAPE=source
E2E_MEDIA_INGEST_MODE=webcodecs_ws
```

Must prove:

- WebCodecs frames reach server;
- FFmpeg publishes stable H.264 RTMP output;
- original audio is present and not delayed;
- provider visible-live/VOD exists.

### Shape 2 — WebCodecs translated output

Purpose: prove WebCodecs video does not degrade STT/TTS.

```bash
E2E_TEST_SHAPE=translated
E2E_SOURCE_LANG=en
E2E_YOUTUBE_LANG=ko
```

Must prove:

- translated TTS completes;
- TTS p95 delay and drift stay inside thresholds;
- video stable;
- no audio stale drops.

### Shape 3 — dual output

Purpose: one host ingest feeds source/pass + translated output.

```bash
E2E_TEST_SHAPE=dual
```

Best for comparing:

- original audio/video timing;
- translated TTS timing;
- per-platform behavior if YouTube + Grip both enabled.

### Shape 4 — congestion drill

Purpose: test shared WS backpressure.

Add:

```bash
E2E_NETWORK_PROFILE=normal|moderate_uplink|severe_uplink
E2E_CPU_STRESS=false|true
E2E_EXPECT_VIDEO_DROPS=false|true
```

Pass can allow video drops under throttle, but not audio/STT/TTS degradation beyond thresholds.

## Browser matrix

Primary evidence:

```text
Brave stable, headed or headless matching operator reality
Chromium stable, headless automation sanity
```

Compatibility evidence:

```text
Firefox stable, WebRTC baseline
Firefox WebCodecs only if browser APIs work and visible-live/VOD passes
Zen headed/Xvfb, compatibility only
```

Never mix Firefox/Zen results into the Chrome/Brave default-promotion decision unless separately proven.

## Artifact schema additions

Add to `meta.json`:

```json
{
  "mediaIngestMode": "webcodecs_ws",
  "requestedMediaIngestMode": "webcodecs_ws",
  "resolvedMediaIngestMode": "webcodecs_ws",
  "webCodecsFrontendEnabled": true,
  "webCodecsServerAdvertised": true,
  "platformMatrix": ["youtube", "grip"],
  "networkProfile": "normal"
}
```

Add to `summary.json`:

```json
{
  "media_ingest_mode": "webcodecs_ws",
  "frontend": {
    "capture_fps_p50": 30,
    "outbound_fps_p50": null,
    "webcodecs_sent_frames": 18000,
    "webcodecs_dropped_frames": 0,
    "webcodecs_server_accepted_frames": 17995,
    "webcodecs_ws_buffered_mb_p95": 0.4,
    "webcodecs_queue_ms_p95": 100,
    "webrtc_disconnects": 0
  },
  "aws_media": {
    "webcodecs_first_keyframe_sec": 1.2,
    "webcodecs_frame_gaps": 0,
    "ffmpeg_speed_min_after_warmup": 0.99,
    "ffmpeg_restarts": 0
  },
  "audio_safety": {
    "host_audio_stale_chunks_dropped": 0,
    "stt_final_delay_ms_p95": 900,
    "tts_delay_ms_p95": 5000,
    "tts_drift_ms_per_min": 100,
    "regression_vs_webrtc_pct": 3.5
  }
}
```

Add to comparison `summary.json`:

```json
{
  "result": "pass",
  "run_count": 2,
  "modes": [
    {
      "media_ingest_mode": "webrtc",
      "provider_confirmed_live_ratio_min": 0.98,
      "ffmpeg_speed_min_after_warmup": 0.99,
      "tts_delay_ms_p95": 5200,
      "failed_gates": []
    },
    {
      "media_ingest_mode": "webcodecs_ws",
      "provider_confirmed_live_ratio_min": 0.99,
      "webcodecs_sent_frames": 18000,
      "webcodecs_dropped_frames": 0,
      "webcodecs_server_accepted_frames": 17995,
      "tts_delay_ms_p95": 5000,
      "failed_gates": []
    }
  ],
  "decision_hint": "webcodecs_ws_candidate"
}
```

## CloudWatch/session-log parsing additions

Parse these events:

```text
frontend.media_ingest_mode_selected
frontend.webcodecs_start
frontend.webcodecs_stats
frontend.webcodecs_video_drop
frontend.webcodecs_issue
server.capabilities
server.video_ingest_selected
server.webcodecs_video_start
server.webcodecs_video_first_keyframe
server.webcodecs_video_frame_gap
server.webcodecs_stats
server.video_ingest_stopped
```

Also preserve WebRTC comparison events:

```text
frontend.media_stats
frontend.webrtc_issue
server.client_media_stats
webrtc VP8/H.264 video track started
webrtc video fps corrected from RTP timestamps
webrtc video dimensions corrected from H.264 SPS
```

## Pass/fail thresholds

Default smoke thresholds:

| Metric | Threshold |
| --- | --- |
| Record duration | >= 95% requested |
| Provider confirmed live ratio | >= 0.90 |
| FFmpeg restarts | 0 |
| FFmpeg exit count | 0 |
| FFmpeg speed min after warmup | >= 0.98 |
| WebCodecs server accepted frames | > 0 |
| WebCodecs normal dropped frames | 0 or <= 0.1% |
| WebCodecs WS buffered p95 | < 4 MB |
| WebCodecs queue p95 | < 500 ms |
| Host audio stale drops | 0 |
| TTS completion ratio | >= 0.95 |
| TTS delay p95 | <= configured max (default 10s) |
| TTS drift | <= configured max (default 500 ms/min) |
| Cloudflare Worker exceptions | 0 |
| YouTube/Grip API errors | 0 outside known quota/outage |

A/B promotion threshold:

- WebCodecs must be equal or better on setup success, visible-live ratio, FFmpeg stability, FPS consistency, and root-cause observability.
- WebCodecs must not regress audio/STT/TTS by >10% versus paired WebRTC baseline.
- If WebCodecs only wins video but worsens audio, do not promote; build split video WS first.

## Run commands

### Single YouTube WebRTC baseline

```bash
E2E_BROWSER=brave \
E2E_MEDIA_INGEST_MODE=webrtc \
E2E_TEST_SHAPE=translated \
E2E_RECORD_SECONDS=600 \
bun run test:e2e:prod-media-stress
```

### Single YouTube WebCodecs run

```bash
E2E_BROWSER=brave \
E2E_MEDIA_INGEST_MODE=webcodecs_ws \
E2E_TEST_SHAPE=translated \
E2E_RECORD_SECONDS=600 \
bun run test:e2e:prod-media-stress
```

### Compare two completed runs

```bash
bun run compare:e2e:media-ingest -- \
  tmp/prod-media-ingest-ab-runs/20260509-youtube-brave \
  tmp/prod-media-stress-runs/<webrtc-run> \
  tmp/prod-media-stress-runs/<webcodecs-run>
```

### Future full matrix wrapper

```bash
E2E_BROWSER_MATRIX=brave,chromium \
E2E_MEDIA_INGEST_MATRIX=webrtc,webcodecs_ws \
E2E_PLATFORM_MATRIX=youtube,grip \
E2E_TEST_SHAPE=translated \
E2E_RECORD_SECONDS=600 \
node scripts/prod-media-ingest-ab-e2e.mjs
```

## Failure triage map

| Symptom | Likely layer | Evidence to inspect | Action |
| --- | --- | --- | --- |
| WebCodecs option disabled | frontend/server flag/capability | setup screenshot, `server:capabilities` | verify deployed env flags |
| `video:webcodecs_start` no ready | server rejected/start bug | host-ws, CloudWatch, session logs | keep flags off; fix parser/state |
| server accepted 0 frames | browser encoder/send path | frontend stats, host console | inspect VideoEncoder/config/support |
| first keyframe missing | encoder/keyframe policy | frontend keyFrames, server logs | force keyframe on start/interval |
| FFmpeg fails on VP8 | IVF/input format | CloudWatch ffmpeg stderr | fix IVF header/frame wrapping |
| YouTube live but Grip not | platform RTMP/librtmp | per-platform FFmpeg logs/evidence | Grip-specific runbook |
| WebCodecs video stable, TTS late | shared WS/audio backpressure | WS buffered, audio stale, TTS delay | split video WS before promotion |
| WebRTC fails, WebCodecs passes | ICE/TURN/WebRTC layer | ICE/candidate logs | WebCodecs candidate for Chrome/Brave |

## Implementation tasks

- [ ] Add mode assertion gates in runner.
- [ ] Add WebCodecs event extraction to analyzer CloudWatch/session-log parser.
- [ ] Add `webcodecs_queue_ms_p95` and better bufferedAmount percentile extraction.
- [ ] Add paired-run wrapper or matrix mode.
- [ ] Add Grip Level 1 manual RTMP/evidence support.
- [ ] Add Grip Level 2 API polling when credentials/API are available.
- [ ] Add network throttling/CPU stress knobs.
- [ ] Add decision-hint output in comparison summary.
- [ ] Add final dated A/B decision template.

## Done when

- A single command can run WebRTC vs WebCodecs with the same fixture/settings.
- `summary.json` and `verdict.md` prove the requested/resolved ingest mode.
- WebCodecs runs fail if capabilities/start/server frames are missing.
- Paired comparison shows platform health, FFmpeg health, WebCodecs counters, and audio/STT/TTS deltas.
- YouTube and Grip artifacts are normalized enough to compare modes.
- Congestion runs can answer whether one WS is safe or split video WS is required.
