#!/usr/bin/env bash
# One-command local E2E runner for the live host flow.
#
# What it automates:
#   - generate Chromium fake camera/mic fixtures if missing
#   - start a local RTMP sink (MediaMTX) unless one is already configured
#   - boot the full local dev stack via scripts/dev-all.sh
#   - run the real-stack Playwright test with Chromium fake-media flags
#   - tear everything down when finished
#
# Default is intentionally cheap/offline-ish: local RTMP only, no voice clone.
# For real YouTube/Grip credentials, pass the same flags supported by
# scripts/test-e2e-real.sh and provide tests/e2e/.env or env vars.

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
LOG_DIR="$ROOT/.dev-logs"
FIXTURE_DIR="$ROOT/tests/e2e/fixtures"
FIXTURE_VIDEO="$FIXTURE_DIR/fake-cam.y4m"
FIXTURE_AUDIO="$FIXTURE_DIR/fake-mic.wav"
DEV_LOG="$LOG_DIR/e2e-auto-dev-stack.log"
RTMP_LOG="$LOG_DIR/e2e-auto-rtmp.log"

mkdir -p "$LOG_DIR" "$FIXTURE_DIR"

SOURCE_LANG="${E2E_SOURCE_LANG:-ko}"
DEST_FLAG_SET=false
DURATION="${E2E_RECORD_SECONDS:-45}"
CLONE_FLAG="--no-clone"
HEADED=""
PW_ARGS=()
FORWARD_ARGS=()
START_LOCAL_RTMP="auto"
GENERATE_FIXTURES="auto"

usage() {
  cat <<'USAGE'
Usage:
  ./scripts/test-e2e-auto.sh [flags] [-- playwright_args...]

Fast default:
  Starts local dev stack + local RTMP sink and runs a fake camera/mic live-flow test:
    --no-clone --source ko --youtube ko --duration 45

Common flags:
  --source <en|ko|ja|zh>     Source language. Default: ko
  --duration <sec>           Live recording duration. Default: 45
  --clone                    Exercise ElevenLabs voice clone instead of skipping it
  --headed                   Show Chromium while running
  --grip <lang>              Use real/local Grip creds from env/.env
  --youtube <lang>           Use YOUTUBE_RTMP_URL/KEY from env/.env
  --rtmp <lang>              Use RTMP_URL/KEY from env/.env
  --rtmp2 <lang>             Use RTMP2_URL/KEY from env/.env
  --instagram <lang>         Use IG_RTMP_URL/KEY from env/.env
  --tiktok <lang>            Use TIKTOK_RTMP_URL/KEY from env/.env
  --no-local-rtmp            Do not start MediaMTX or inject local YOUTUBE_RTMP_* creds
  --no-generate-fixtures     Do not generate fake-cam.y4m/fake-mic.wav if missing
  -h, --help                 Show this help

Everything after -- is passed to Playwright.
USAGE
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --source)
      SOURCE_LANG="$2"; FORWARD_ARGS+=("--source" "$2"); shift 2 ;;
    --duration)
      DURATION="$2"; FORWARD_ARGS+=("--duration" "$2"); shift 2 ;;
    --clone)
      CLONE_FLAG=""; shift ;;
    --no-clone)
      CLONE_FLAG="--no-clone"; shift ;;
    --headed)
      HEADED="--headed"; shift ;;
    --grip|--youtube|--rtmp|--rtmp2|--instagram|--tiktok)
      DEST_FLAG_SET=true; FORWARD_ARGS+=("$1" "$2"); shift 2 ;;
    --no-local-rtmp)
      START_LOCAL_RTMP="false"; shift ;;
    --no-generate-fixtures)
      GENERATE_FIXTURES="false"; shift ;;
    -h|--help)
      usage; exit 0 ;;
    --)
      shift; PW_ARGS+=("$@"); break ;;
    *)
      PW_ARGS+=("$1"); shift ;;
  esac
done

case "$SOURCE_LANG" in
  en|ko|ja|zh) ;;
  *) echo "ERROR: --source must be en|ko|ja|zh, got '$SOURCE_LANG'." >&2; exit 1 ;;
esac

have_cmd() { command -v "$1" >/dev/null 2>&1; }

if [[ "$GENERATE_FIXTURES" != "false" && ( ! -f "$FIXTURE_VIDEO" || ! -f "$FIXTURE_AUDIO" ) ]]; then
  if ! have_cmd ffmpeg; then
    echo "ERROR: fake media fixtures missing and ffmpeg is unavailable." >&2
    echo "  Install ffmpeg or run with --no-generate-fixtures after creating fixtures." >&2
    exit 1
  fi
  seconds="${E2E_FIXTURE_SECONDS:-120}"
  echo "==> Generating fake Chromium media fixtures (${seconds}s)"
  # Keep these deliberately small: they prove browser getUserMedia + audio/video
  # transport without writing multi-GB raw 1080p Y4M files.
  ffmpeg -hide_banner -loglevel error -y \
    -f lavfi -i "testsrc2=size=640x360:rate=15" \
    -t "$seconds" -pix_fmt yuv420p "$FIXTURE_VIDEO"
  ffmpeg -hide_banner -loglevel error -y \
    -f lavfi -i "sine=frequency=440:sample_rate=44100" \
    -t "$seconds" -ac 1 -acodec pcm_s16le "$FIXTURE_AUDIO"
fi

DEV_PID=""
RTMP_CONTAINER=""
cleanup() {
  set +e
  if [[ -n "$DEV_PID" ]]; then
    kill "$DEV_PID" 2>/dev/null || true
    wait "$DEV_PID" 2>/dev/null || true
  fi
  if [[ -n "$RTMP_CONTAINER" ]]; then
    docker rm -f "$RTMP_CONTAINER" >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT INT TERM

if [[ "$START_LOCAL_RTMP" != "false" && "$DEST_FLAG_SET" == "false" ]]; then
  if ! have_cmd docker; then
    echo "ERROR: docker is required for the default local RTMP sink." >&2
    echo "  Or pass an explicit --youtube/--rtmp with env creds and --no-local-rtmp." >&2
    exit 1
  fi
  if ! curl -sf --max-time 1 http://localhost:8888 >/dev/null 2>&1 && \
     ! lsof -iTCP:1935 -sTCP:LISTEN >/dev/null 2>&1; then
    RTMP_CONTAINER="brivva-e2e-mediamtx-$$"
    echo "==> Starting local RTMP sink (MediaMTX)"
    docker run --rm --name "$RTMP_CONTAINER" \
      -p 1935:1935 -p 8888:8888 -p 8889:8889 \
      bluenviron/mediamtx:latest >"$RTMP_LOG" 2>&1 &
    for _ in {1..30}; do
      if lsof -iTCP:1935 -sTCP:LISTEN >/dev/null 2>&1; then break; fi
      sleep 1
    done
  else
    echo "==> Reusing existing RTMP sink on localhost"
  fi
  export YOUTUBE_RTMP_URL="${YOUTUBE_RTMP_URL:-rtmp://localhost:1935/live}"
  export YOUTUBE_RTMP_KEY="${YOUTUBE_RTMP_KEY:-brivva-e2e-${SOURCE_LANG}}"
  FORWARD_ARGS+=("--youtube" "$SOURCE_LANG")
fi

if [[ "$DEST_FLAG_SET" == "false" ]]; then
  # Default path above injects local RTMP. If local RTMP was disabled, fail clearly.
  if [[ -z "${YOUTUBE_RTMP_URL:-}" || -z "${YOUTUBE_RTMP_KEY:-}" ]]; then
    echo "ERROR: no destination configured." >&2
    echo "  Use default local RTMP, or pass --youtube/--rtmp/etc with creds." >&2
    exit 1
  fi
fi

echo "==> Starting full local dev stack"
( export DEV_AUTH_BYPASS=true; "$ROOT/scripts/dev-all.sh" ) >"$DEV_LOG" 2>&1 &
DEV_PID=$!

FRONTEND_URL="${BRIVVA_DEV_FRONTEND_URL:-http://localhost:5173}"
WORKERS_URL="${BRIVVA_DEV_WORKERS_URL:-http://localhost:8787}"
SERVER_URL="${BRIVVA_DEV_SERVER_URL:-http://localhost:3000}"
for label_url in "server:$SERVER_URL/health" "workers:$WORKERS_URL/" "frontend:$FRONTEND_URL/"; do
  label="${label_url%%:*}"
  url="${label_url#*:}"
  echo "==> Waiting for $label ($url)"
  ready=false
  for _ in {1..120}; do
    if ! kill -0 "$DEV_PID" 2>/dev/null; then
      echo "ERROR: dev stack exited early. Tail:" >&2
      tail -n 80 "$DEV_LOG" >&2 || true
      exit 1
    fi
    if curl -sf --max-time 2 "$url" >/dev/null 2>&1; then
      ready=true; break
    fi
    sleep 1
  done
  if [[ "$ready" != "true" ]]; then
    echo "ERROR: timed out waiting for $label. Tail:" >&2
    tail -n 80 "$DEV_LOG" >&2 || true
    exit 1
  fi
done

args=()
[[ -n "$CLONE_FLAG" ]] && args+=("$CLONE_FLAG")
args+=("--source" "$SOURCE_LANG" "--duration" "$DURATION")
args+=("${FORWARD_ARGS[@]}")
[[ -n "$HEADED" ]] && args+=("$HEADED")

# Deduplicate --source/--duration if caller already provided them in FORWARD_ARGS.
# Last flag wins in test-e2e-real.sh; this is acceptable and keeps parsing simple.
echo "==> Running Playwright real-stack E2E with fake Chromium media"
"$ROOT/scripts/test-e2e-real.sh" "${args[@]}" -- "${PW_ARGS[@]}"

echo "==> E2E complete"
