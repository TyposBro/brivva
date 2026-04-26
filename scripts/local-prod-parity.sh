#!/usr/bin/env bash
# Build and run the media pipeline in the same container shape as Fargate:
# linux/amd64 server-rs image + source-built ffmpeg-base with librtmp/drawtext.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
COMPOSE_FILE="${REPO_ROOT}/tests/e2e/compose.e2e.yml"

KEEP=false
SKIP_BUILD=false

usage() {
  echo "Usage: $0 [--keep] [--skip-build]"
  echo ""
  echo "  --keep        Leave the compose stack running after smoke passes/fails"
  echo "  --skip-build  Reuse existing local brivva-smoke-* images"
  exit 1
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --keep) KEEP=true; shift ;;
    --skip-build) SKIP_BUILD=true; shift ;;
    -h|--help) usage ;;
    *) echo "Unknown arg: $1"; usage ;;
  esac
done

cleanup() {
  if [[ "$KEEP" == false ]]; then
    docker compose -f "$COMPOSE_FILE" down -v >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT INT TERM

cd "$REPO_ROOT"

if [[ "$SKIP_BUILD" == false ]]; then
  echo "==> Building prod-parity ffmpeg-base (linux/amd64)"
  docker buildx build \
    --platform linux/amd64 \
    --load \
    -t brivva-smoke-ffmpeg-base:local \
    -f infra/ffmpeg-base/Dockerfile infra/ffmpeg-base

  echo "==> Building prod-parity server-build-base (linux/amd64)"
  docker buildx build \
    --platform linux/amd64 \
    --load \
    -t brivva-smoke-server-build-base:local \
    -f infra/server-build-base/Dockerfile infra/server-build-base

  echo "==> Building prod-parity server-runtime-base (linux/amd64)"
  docker buildx build \
    --platform linux/amd64 \
    --load \
    -t brivva-smoke-server-runtime-base:local \
    --build-arg FFMPEG_BASE_IMAGE=brivva-smoke-ffmpeg-base:local \
    -f infra/server-runtime-base/Dockerfile infra/server-runtime-base

  echo "==> Building prod-parity server-rs (linux/amd64)"
  docker buildx build \
    --platform linux/amd64 \
    --load \
    -t brivva-smoke-server:local \
    --build-arg SERVER_BUILD_BASE_IMAGE=brivva-smoke-server-build-base:local \
    --build-arg SERVER_RUNTIME_BASE_IMAGE=brivva-smoke-server-runtime-base:local \
    -f server-rs/Dockerfile .
fi

echo "==> Building test stubs"
docker compose -f "$COMPOSE_FILE" build workers-stub soniox-stub elevenlabs-stub

echo "==> Starting prod-parity smoke stack"
docker compose -f "$COMPOSE_FILE" up --no-build -d

echo "==> Verifying runtime ffmpeg invariants inside server container"
docker compose -f "$COMPOSE_FILE" exec -T server \
  sh -c '/usr/local/bin/ffmpeg -version | grep -q "enable-librtmp"'
docker compose -f "$COMPOSE_FILE" exec -T server \
  sh -c '/usr/local/bin/ffmpeg -hide_banner -filters 2>&1 | grep -q "drawtext"'
docker compose -f "$COMPOSE_FILE" exec -T server \
  sh -c 'test "$(uname -m)" = "x86_64"'

echo "==> Running media pipeline smoke"
(
  cd "$REPO_ROOT/tests/e2e"
  SERVER_URL=http://localhost:3000 \
  WS_URL=ws://localhost:3000/api/session \
  RTMP_URL=rtmp://localhost:1935/live/smoke-ja \
  JWT_SECRET=smoke-jwt-secret \
  SESSION_ID=SMOKE001 \
  USER_ID=smoke-user \
    bun run smoke
)

echo "==> Prod-parity local smoke passed"
if [[ "$KEEP" == true ]]; then
  echo "    Stack left running. RTMP output: rtmp://localhost:1935/live/smoke-ja"
fi
