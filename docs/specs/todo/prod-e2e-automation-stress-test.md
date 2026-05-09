# Prod E2E Media Automation Stress Test Plan

Status: implemented (automation scripts added)
Owner: Brivva engineering
Target env: Cloudflare Pages frontend + Cloudflare Workers API + AWS Rust media server + YouTube Live
Related docs: `docs/2026.05.06-report.md`, `docs/browser-webrtc-ingest-roadmap.md`, `docs/launch-browser-and-media-runbook.md`, `docs/rust-stress-test/tracker.md`

## Goal

Automate a production stress test that:

1. Opens the real production frontend (`https://brivva.pages.dev`).
2. Creates real production sessions through Workers (`https://brivva-api.milliytechnology.workers.dev`).
3. Streams local fixture media from `~/Desktop/text.mp4` through a real browser host into the AWS Rust WebRTC/RTMP media path.
4. Creates/uses YouTube Live broadcasts and watches them from browser watcher pages.
5. Measures smoothness and correctness of:
   - original video;
   - original/source audio;
   - translated TTS audio;
   - resolution, FPS, bitrate/quality, frame drops, FFmpeg speed;
   - TTS delay vs original/source speech and drift over time.
6. Captures observability from:
   - Cloudflare Workers Observability / tail logs;
   - Cloudflare Pages browser console + session logs;
   - Workers D1/session endpoints;
   - AWS CloudWatch `/ecs/brivva` logs;
   - YouTube provider health and watch/VOD evidence.

This is **not** a mock/local test. It is a controlled production E2E rehearsal.

## Non-goals

- Do not leak YouTube/RTMP stream keys or OAuth tokens into artifacts.
- Do not use fake RTMP sinks as final proof. Fake sink can be a preflight only.
- Do not mutate shared prod secrets for bad-provider drills.
- Do not claim Firefox/Zen support from Chromium-only fake-device behavior.
- Do not call internal FFmpeg `output.live` equivalent to YouTube visible-live unless YouTube confirms ingest/live status.

## Existing baseline

Current maintained-ish script:

```text
scripts/prod-youtube-oauth-e2e.mjs
```

It already:

- creates a prod YouTube OAuth auto-broadcast session;
- opens prod Pages;
- uses Playwright Chromium fake camera/audio flags;
- records host/watcher screenshots;
- fetches session logs;
- exports AWS CloudWatch logs.

Needed upgrade: turn it into a browser/media stress harness with fixture injection, browser matrix, multi-output watch, objective metrics, Cloudflare Observability capture, and `summary.json` pass/fail gates.

Implemented automation entrypoints:

```bash
node scripts/prod-media-stress-e2e.mjs
node scripts/analyze-prod-media-stress.mjs tmp/prod-media-stress-runs/<run-id>
```

Package aliases:

```bash
bun run test:e2e:prod-media-stress
bun run analyze:e2e:prod-media-stress -- tmp/prod-media-stress-runs/<run-id>
```

## Browser matrix

Implement runner flag:

```bash
BROWSER=chromium|brave|firefox|zen
HEADLESS=true|false
```

### Chromium / Brave

Primary path.

Options:

1. Keep current Chromium fake-device flags after converting MP4 to Y4M/WAV.
2. Prefer new cross-browser media shim below, because it lets all browsers use the same `text.mp4` source.

Brave support:

```bash
BROWSER=brave BROWSER_EXECUTABLE=/usr/bin/brave-browser HEADLESS=true \
  E2E_MEDIA_FILE=$HOME/Desktop/text.mp4 \
  node scripts/prod-media-stress-e2e.mjs
```

### Firefox

Use to validate VP8/TURN/cross-browser ingest.

Caveat: Firefox does not support Chromium's `--use-file-for-fake-video-capture` path. Use the media shim or virtual devices. Headless Firefox may differ from desktop Firefox/Zen, so mark results separately.

### Zen

Zen is Firefox-based and may need headed/Xvfb or a persistent profile. Treat as compatibility evidence only unless automation proves the actual browser path.

Example:

```bash
BROWSER=zen BROWSER_EXECUTABLE=/path/to/zen HEADLESS=false \
  E2E_MEDIA_FILE=$HOME/Desktop/text.mp4 \
  node scripts/prod-media-stress-e2e.mjs
```

If Playwright cannot control Zen reliably, fallback path is Xvfb + virtual camera/audio devices.

## Fixture strategy

Input source:

```text
~/Desktop/text.mp4
```

### Preferred: browser media shim

Implement a test-only Playwright `addInitScript` before loading prod Pages:

1. Start a local static fixture server on `127.0.0.1:<port>` that serves `text.mp4` with range requests.
2. Inject an override for `navigator.mediaDevices.getUserMedia`.
3. The override creates a hidden `<video>` element with `src=http://127.0.0.1:<port>/text.mp4`, `loop=true`, `autoplay=true`, and `playsInline=true`.
4. Use `video.captureStream()` / `mozCaptureStream()` to return deterministic video/audio tracks.
5. Respect requested constraints:
   - video-only request returns video tracks;
   - audio-only request returns audio tracks;
   - audio+video request returns both.
6. Log track settings and capture lifecycle to browser console with `run_id`.

Why this path: one MP4 works across Chromium, Brave, Firefox, and possibly Zen. It also avoids maintaining separate Y4M/WAV derivatives.

### Fallback: Chromium fake-device conversion

If media shim breaks, convert once:

```bash
mkdir -p tmp/prod-media-fixtures
ffmpeg -y -i "$HOME/Desktop/text.mp4" \
  -vf "scale=720:1280:force_original_aspect_ratio=decrease,pad=720:1280:(ow-iw)/2:(oh-ih)/2,fps=30" \
  -pix_fmt yuv420p tmp/prod-media-fixtures/text-720x1280-30.y4m

ffmpeg -y -i "$HOME/Desktop/text.mp4" \
  -vn -ar 44100 -ac 1 -sample_fmt s16 tmp/prod-media-fixtures/text-audio-44k-mono.wav
```

Use with Chromium flags:

```text
--use-file-for-fake-video-capture=tmp/prod-media-fixtures/text-720x1280-30.y4m
--use-file-for-fake-audio-capture=tmp/prod-media-fixtures/text-audio-44k-mono.wav
```

## Test shapes

### Shape A — source/pass baseline

Purpose: measure original video + original audio without translation load.

- Source lang: env `E2E_SOURCE_LANG`, default `en`.
- Destination: YouTube pass/source output.
- Duration: 10 min smoke, then 30–60 min soak.
- Host gain: 1.0.

Pass checks:

- YouTube visible live.
- VOD exists after stop.
- Source audio audible and not drifting from video.
- FFmpeg speed stable near realtime.
- No FFmpeg crash/restart.

### Shape B — translated TTS output

Purpose: measure TTS quality/delay/drift under real STT/TTS load.

- Source lang: env `E2E_SOURCE_LANG`, default `en`.
- Target lang: env `E2E_YOUTUBE_LANG`, default `ko`.
- Destination: YouTube translated output.
- Host gain: 0.2 by default, configurable.
- Duration: 10 min smoke, then 30–60 min soak.

Pass checks:

- Source video remains smooth.
- Original audio remains present under translated lane.
- TTS is audible, intelligible, and near-live.
- TTS delay p95 stays within configured threshold.
- TTS drift slope does not grow over the run.
- No systemic `tts segment queue overflow`.

### Shape C — dual-output one-session proof

Purpose: compare source/pass and translated output from the same host run.

- Create two YouTube outputs:
  - pass/source output;
  - translated output.
- Watch both YouTube URLs in separate watcher pages.
- Poll health for both stream IDs.

This is the best objective run if YouTube quota and app flow allow multiple auto-created YouTube broadcasts in one session.

## Runner design

Create:

```text
scripts/prod-media-stress-e2e.mjs
scripts/analyze-prod-media-stress.mjs
```

Or refactor existing:

```text
scripts/prod-youtube-oauth-e2e.mjs
```

Required runner flags/env:

```bash
BRIVVA_PROD_FRONTEND_URL=https://brivva.pages.dev
BRIVVA_PROD_API_URL=https://brivva-api.milliytechnology.workers.dev
E2E_USER_ID=111778147327359525059
E2E_MEDIA_FILE=$HOME/Desktop/text.mp4
E2E_SOURCE_LANG=en
E2E_YOUTUBE_LANG=ko
E2E_RECORD_SECONDS=1800
E2E_BROWSER=brave
E2E_HEADLESS=true
E2E_TEST_SHAPE=dual|source|translated
E2E_FETCH_CLOUDWATCH=true
E2E_FETCH_CF_OBSERVABILITY=true
E2E_CLEANUP_SESSION=false
```

Run folder:

```text
tmp/prod-media-stress-runs/YYYYMMDD-HHMMSS-<browser>-<shape>/
  meta.json
  input-media.ffprobe.json
  created.redacted.json
  watch-urls.json
  host-console.log
  watcher-console-<stream>.log
  host-video.webm
  watcher-video-<stream>.webm
  screenshots/
  host-body-final.txt
  watch-body-final-<stream>.txt
  session-logs.ndjson
  provider-health.ndjson
  session-summary.json
  session-usage.json
  cloudwatch-all.json
  cloudwatch-filtered.ndjson
  cf-worker-tail.ndjson
  cf-observability-summary.json
  youtube-vod-metadata.json
  youtube-vod-analysis.json
  summary.json
  verdict.md
```

## Observability capture

### Browser / Pages

Capture from host page:

- console logs;
- page errors;
- screenshots every 30s;
- Playwright video recording;
- body text snapshots;
- app media stats shown in UI;
- `session-logs.ndjson` from Workers session-log endpoint.

Pages itself has no server code here, so the browser console/session logs are the main frontend observability artifact.

### Workers / Cloudflare Observability

Before session creation, start a Cloudflare tail process if credentials exist:

```bash
wrangler tail brivva-api --format=json --sampling-rate=1 > cf-worker-tail.ndjson
```

Filter by:

- `run_id` in session title / translation terms;
- `session_id`;
- `stream_id`;
- YouTube broadcast ID.

Capture:

- session create path;
- YouTube broadcast/stream/bind calls;
- `/api/sessions/:id/provider-health` polling;
- D1 write/read errors;
- internal metrics updates;
- provider failure window persistence;
- billing summary/usage calls;
- Worker exceptions/subrequest failures.

If using the Cloudflare Observability dashboard/API instead of `wrangler tail`, export a summary artifact with:

```json
{
  "worker_errors": 0,
  "worker_exceptions": 0,
  "youtube_api_errors": 0,
  "d1_errors": 0,
  "provider_health_polls": 0,
  "session_log_writes": 0
}
```

Implementation note: verify current Cloudflare Observability/GraphQL API syntax against Cloudflare docs before hardcoding API queries.

### AWS CloudWatch

Use existing pattern from `scripts/prod-youtube-oauth-e2e.mjs`:

```bash
aws logs filter-log-events \
  --region us-east-1 \
  --log-group-name /ecs/brivva \
  --start-time <run_start_ms_minus_60s> \
  --end-time <run_end_ms_plus_60s> \
  --output json > cloudwatch-all.json
```

Filter into `cloudwatch-filtered.ndjson` by:

- `session_id`;
- `live_session_id`;
- `stream_id`;
- `output_id`;
- `ffmpeg`;
- `webrtc`;
- `provider_health`;
- `tts`;
- `soniox`;
- `elevenlabs`.

Extract metrics:

- FFmpeg start/restart/exit count;
- `speed=` min/p50/p95;
- dropped frames;
- `video_stale_chunks_dropped`;
- `host_audio_stale_chunks_dropped`;
- `ready_host_bytes_dropped`;
- WebRTC codec, dimensions, FPS;
- SPS-observed dimensions;
- RTMP output profile;
- provider health state transitions;
- TTS dispatch/complete/end/video-end timestamps;
- TTS queue overflow and hard recovery counts.

## Provider health polling

During the run, poll every 10s:

```text
GET /api/sessions/:id/provider-health
GET /api/sessions/:id/summary
GET /api/sessions/:id/usage
```

Write newline-delimited artifacts:

```text
provider-health.ndjson
session-summary-samples.ndjson
session-usage-samples.ndjson
```

YouTube pass condition is provider-confirmed live:

```json
{
  "provider": "youtube",
  "streamStatus": "active",
  "healthStatus": "good",
  "providerConfirmedLive": true
}
```

If internal FFmpeg says live but YouTube stream status is inactive/finalized, mark the run failed.

## YouTube watcher automation

For every YouTube watch URL:

1. Open watcher page before host clicks Record.
2. Screenshot before/after live starts.
3. Keep watcher page foreground or disable background throttling.
4. Record watcher page video.
5. Poll body text for live/offline/error signals.
6. Take screenshots every 30s.
7. After Stop, wait for VOD processing and reload.
8. Save final watch body and screenshot.

Optional but valuable:

- Attempt VOD download with `yt-dlp` after processing.
- Run `ffprobe` on downloaded VOD.
- Compare VOD duration against expected record duration.
- Detect frame cadence gaps from frame PTS.
- Extract audio and compare source/pass audio timing against input fixture markers.

## Timing / drift model

Use two levels.

### Level 1 — app/server timing

Parse `session-logs.ndjson` + CloudWatch for per-utterance fields:

- source utterance start/end media time;
- STT final time;
- translation emitted time;
- TTS request start/end;
- TTS PCM queued time;
- translated audio playout/video-end time;
- target lang;
- utterance id.

Compute:

```text
tts_delay_ms = translated_audio_playout_start_ms - source_utterance_end_ms
tts_drift_ms = tts_delay_ms_at_end - tts_delay_ms_at_start
tts_drift_slope_ms_per_min = drift / run_minutes
```

If current logs do not expose enough fields, add structured session log events before relying on audio-only inference.

### Level 2 — VOD/media timing

For source/pass output:

- extract source fixture audio markers;
- extract YouTube VOD audio markers;
- compare marker timestamps;
- compare video frame PTS/cadence.

For translated output:

- use app/server TTS timestamps as source of truth;
- optionally align VOD TTS audio energy/onsets with logged TTS utterance windows.

Best future fixture: generate a derivative of `text.mp4` with a visible timecode and short beep every 10s. That makes original audio/video drift objectively measurable after YouTube re-encoding.

## Summary schema

`summary.json` should be machine-readable:

```json
{
  "result": "pass",
  "run_id": "20260509-120000-brave-dual",
  "browser": "brave",
  "headless": true,
  "shape": "dual",
  "duration_sec": 1800,
  "session_id": "...",
  "live_session_id": "...",
  "streams": [
    {
      "platform": "youtube",
      "kind": "source",
      "watch_url": "https://www.youtube.com/watch?v=...",
      "provider_confirmed_live_sec": 1790,
      "health_status_p95": "good",
      "vod_duration_sec": 1792,
      "vod_width": 720,
      "vod_height": 1280,
      "vod_fps": 30
    }
  ],
  "frontend": {
    "capture_width": 720,
    "capture_height": 1280,
    "capture_fps_p50": 30,
    "outbound_width": 720,
    "outbound_height": 1280,
    "outbound_fps_p50": 30,
    "webrtc_disconnects": 0
  },
  "aws_media": {
    "ffmpeg_restarts": 0,
    "ffmpeg_exit_count": 0,
    "ffmpeg_speed_min_after_warmup": 0.98,
    "ffmpeg_speed_p50": 1.0,
    "video_stale_chunks_dropped": 0,
    "host_audio_stale_chunks_dropped": 0,
    "ready_host_bytes_dropped": 0,
    "output_profile": "720x1280@30 h264 nvenc 2500k"
  },
  "tts": {
    "utterances_source": 120,
    "utterances_tts_completed": 120,
    "segment_overflows": 0,
    "hard_recovery": 0,
    "delay_ms_p50": 3200,
    "delay_ms_p95": 5500,
    "drift_ms_per_min": 50
  },
  "cloudflare": {
    "worker_errors": 0,
    "d1_errors": 0,
    "youtube_api_errors": 0,
    "session_log_write_errors": 0
  },
  "artifacts_dir": "tmp/prod-media-stress-runs/..."
}
```

## Pass/fail gates

Default smoke gate:

- session created successfully;
- browser reaches live page and records for full duration;
- every expected YouTube stream has `providerConfirmedLive=true` for at least 90% of run;
- VOD/watch page exists after stop;
- no FFmpeg crash/restart;
- no sustained below-realtime encode after warmup;
- no normal-run media stale drops;
- no WebRTC failed/closed before Stop;
- TTS utterance completion ratio >= 95%;
- `tts segment queue overflow=0` unless stress mode explicitly allows it;
- TTS p95 delay <= configured threshold;
- TTS drift slope <= configured threshold;
- Cloudflare Worker exceptions = 0;
- AWS CloudWatch export succeeds.

Default soak gate:

- all smoke gates;
- duration 30–60 min;
- YouTube visible-live screenshots/video throughout;
- source/pass output audible and smooth;
- translated output TTS intelligible by human spot-check;
- billing/usage summary available and sane;
- no provider health false-positive where Brivva says live but YouTube finalized/offline.

## Implementation phases

### Phase 1 — Refactor current prod YouTube script

- Add `E2E_BROWSER`, `E2E_TEST_SHAPE`, `E2E_MEDIA_FILE`.
- Keep Chromium fake-device path working.
- Add multi-watch support for multiple YouTube streams.
- Poll provider health/summary/usage during run.
- Produce `summary.json` and `verdict.md`.

### Phase 2 — Cross-browser media shim

- Implement local fixture server.
- Inject getUserMedia override before app load.
- Validate Chromium/Brave first.
- Validate Firefox next.
- Attempt Zen headed/Xvfb last.

### Phase 3 — Observability exporters

- Wrap `wrangler tail` for Workers logs.
- Keep AWS CloudWatch export.
- Add Cloudflare Observability summary artifact.
- Redact secrets everywhere.

### Phase 4 — Analyzer

- Parse session logs, CloudWatch, provider health, CF tail.
- Compute FPS/resolution/speed/drop/restart/TTS delay/drift metrics.
- Optionally download and analyze YouTube VOD.

### Phase 5 — Soak and browser matrix

Run matrix:

```text
brave/chromium source 10m
brave/chromium translated 10m
brave/chromium dual 30-60m
firefox translated 10m
zen translated 10m headed/Xvfb if possible
```

Only promote Firefox/Zen from watch to supported after visible-live + VOD + logs pass.

## Example target commands

Chromium smoke:

```bash
E2E_BROWSER=chromium \
E2E_HEADLESS=true \
E2E_TEST_SHAPE=translated \
E2E_MEDIA_FILE=$HOME/Desktop/text.mp4 \
E2E_RECORD_SECONDS=600 \
node scripts/prod-media-stress-e2e.mjs
```

Brave dual soak:

```bash
E2E_BROWSER=brave \
BROWSER_EXECUTABLE=/usr/bin/brave-browser \
E2E_HEADLESS=true \
E2E_TEST_SHAPE=dual \
E2E_MEDIA_FILE=$HOME/Desktop/text.mp4 \
E2E_RECORD_SECONDS=3600 \
E2E_FETCH_CLOUDWATCH=true \
E2E_FETCH_CF_OBSERVABILITY=true \
node scripts/prod-media-stress-e2e.mjs
```

Firefox compatibility:

```bash
E2E_BROWSER=firefox \
E2E_HEADLESS=true \
E2E_TEST_SHAPE=translated \
E2E_MEDIA_FILE=$HOME/Desktop/text.mp4 \
E2E_RECORD_SECONDS=600 \
node scripts/prod-media-stress-e2e.mjs
```

## Risks / blind spots

- Headless browser media behavior can differ from real host desktop behavior.
- YouTube watch page text scraping is noisy; screenshots/VOD/provider API are stronger.
- YouTube VOD processing can lag; analyzer needs retry/backoff.
- Firefox/Zen automation may need media shim or virtual devices.
- Local runner network can become the bottleneck; log candidate pair/TURN info and uplink stats.
- TTS drift cannot be objectively measured from mixed YouTube audio unless server emits structured per-utterance timing or fixture markers are added.
- Cloudflare Observability API details may change; verify against current docs when implementing exporter.

## Done when

- A single command can run a prod media stress test from `~/Desktop/text.mp4`.
- Artifacts include browser, Workers, AWS, YouTube, and summary outputs.
- `summary.json` fails CI/operator gate on bad FPS, bad resolution, FFmpeg restart, provider health mismatch, TTS overflow, or drift.
- Brave/Chromium prod run passes 30–60 min exact launch-profile soak.
- Firefox/Zen results are recorded separately with honest support status.
