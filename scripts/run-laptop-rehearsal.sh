#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
LOG_DIR="${REPO_ROOT}/.dev-logs"
mkdir -p "$LOG_DIR"

MEDIA_URL="${MEDIA_URL:-http://127.0.0.1:3000}"
FRONTEND_PORT="${FRONTEND_PORT:-5173}"
FRONTEND_URL="${FRONTEND_URL:-http://localhost:${FRONTEND_PORT}}"
WORKERS_API_URL="${WORKERS_API_URL:-https://brivva-api.milliytechnology.workers.dev}"

usage() {
	cat <<EOF
Usage: $0 [--preflight-only]

Start laptop GPU media engine + local frontend wired to it.
Use this while ECS GPU is blocked by quota and for May 10 hot fallback drill.

Env overrides:
  MEDIA_URL=http://127.0.0.1:3000
  FRONTEND_PORT=5173
  FRONTEND_URL=http://localhost:5173
  WORKERS_API_URL=https://brivva-api.milliytechnology.workers.dev
EOF
	exit 1
}

PREFLIGHT_ONLY=false
while [[ $# -gt 0 ]]; do
	case "$1" in
	--preflight-only)
		PREFLIGHT_ONLY=true
		shift
		;;
	-h | --help) usage ;;
	*)
		echo "Unknown arg: $1" >&2
		usage
		;;
	esac
done

need() {
	if ! command -v "$1" >/dev/null 2>&1; then
		echo "Missing required command: $1" >&2
		exit 1
	fi
}
need bun
need curl

cleanup() {
	if [[ -n "${ENGINE_PID:-}" ]]; then kill "$ENGINE_PID" 2>/dev/null || true; fi
	if [[ -n "${FRONTEND_PID:-}" ]]; then kill "$FRONTEND_PID" 2>/dev/null || true; fi
}
trap cleanup EXIT INT TERM

BRIVVA_VIDEO_ENCODER=nvenc \
	WORKERS_API_URL="$WORKERS_API_URL" \
	FRONTEND_URL="$FRONTEND_URL" \
	"$REPO_ROOT/scripts/run-local-engine.sh" --preflight-only

if [[ "$PREFLIGHT_ONLY" == true ]]; then
	echo "✓ laptop rehearsal preflight complete"
	exit 0
fi

if lsof -iTCP:"$FRONTEND_PORT" -sTCP:LISTEN >/dev/null 2>&1; then
	echo "Frontend port $FRONTEND_PORT already in use" >&2
	lsof -iTCP:"$FRONTEND_PORT" -sTCP:LISTEN >&2
	exit 2
fi

BRIVVA_VIDEO_ENCODER=nvenc \
	WORKERS_API_URL="$WORKERS_API_URL" \
	FRONTEND_URL="$FRONTEND_URL" \
	"$REPO_ROOT/scripts/run-local-engine.sh" >"$LOG_DIR/laptop-rehearsal-engine.log" 2>&1 &
ENGINE_PID=$!

for _ in {1..60}; do
	if curl -fsS "$MEDIA_URL/health" >/dev/null 2>&1; then
		break
	fi
	if ! kill -0 "$ENGINE_PID" 2>/dev/null; then
		echo "Engine exited early; tail log:" >&2
		tail -80 "$LOG_DIR/laptop-rehearsal-engine.log" >&2
		exit 3
	fi
	sleep 1
done

"$REPO_ROOT/scripts/smoke-media-engine.sh" "$MEDIA_URL"

(
	cd "$REPO_ROOT/frontend"
	VITE_API_URL="$WORKERS_API_URL" \
		VITE_MEDIA_URL="$MEDIA_URL" \
		VITE_WORKER_URL="$MEDIA_URL" \
		bun run dev --host 0.0.0.0 --port "$FRONTEND_PORT"
) >"$LOG_DIR/laptop-rehearsal-frontend.log" 2>&1 &
FRONTEND_PID=$!

for _ in {1..60}; do
	if curl -fsS "$FRONTEND_URL" >/dev/null 2>&1; then
		break
	fi
	if ! kill -0 "$FRONTEND_PID" 2>/dev/null; then
		echo "Frontend exited early; tail log:" >&2
		tail -80 "$LOG_DIR/laptop-rehearsal-frontend.log" >&2
		exit 4
	fi
	sleep 1
done

cat <<EOF
✓ laptop rehearsal stack ready
frontend_url=$FRONTEND_URL
media_url=$MEDIA_URL
workers_api_url=$WORKERS_API_URL
engine_log=$LOG_DIR/laptop-rehearsal-engine.log
frontend_log=$LOG_DIR/laptop-rehearsal-frontend.log

Next manual step:
  Open $FRONTEND_URL, start a host session, stream 30s+, then run:
  ./scripts/verify-session-burn-in.sh <SESSION_ID> --remote

Press Ctrl-C to stop engine + frontend.
EOF

wait
