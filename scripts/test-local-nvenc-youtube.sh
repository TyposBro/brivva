#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
LOG_DIR="$REPO_ROOT/.dev-logs"
ENV_FILE="$REPO_ROOT/tests/e2e/.env"
DURATION_SECONDS="${DURATION_SECONDS:-${1:-3600}}"
SOURCE_LANG="${SOURCE_LANG:-en}"
DEST_LANG="${DEST_LANG:-$SOURCE_LANG}"
INFISICAL_ENV="${INFISICAL_ENV:-prod}"
BOOT_TIMEOUT="${BOOT_TIMEOUT:-180}"
mkdir -p "$LOG_DIR"

usage() {
	cat <<EOF
Usage: $0 [duration_seconds]

Runs a fully local NVENC browser burn-in with fake camera/mic to YouTube RTMP.
No ECS/prod deploy. Requires YouTube RTMP creds in tests/e2e/.env or env:
  YOUTUBE_RTMP_URL
  YOUTUBE_RTMP_KEY

Env:
  DURATION_SECONDS=3600
  SOURCE_LANG=en
  DEST_LANG=
  INFISICAL_ENV=prod
EOF
	exit 1
}

case "${1:-}" in
-h | --help) usage ;;
esac

need() {
	if ! command -v "$1" >/dev/null 2>&1; then
		echo "Missing required command: $1" >&2
		exit 1
	fi
}
need bun
need curl
need lsof
need node

if [[ -f "$ENV_FILE" ]]; then
	# shellcheck disable=SC1090
	set -a
	. "$ENV_FILE"
	set +a
fi
if [[ -z "${YOUTUBE_RTMP_URL:-}" || -z "${YOUTUBE_RTMP_KEY:-}" || "${YOUTUBE_RTMP_KEY:-}" == YOUR_* ]]; then
	echo "Missing YOUTUBE_RTMP_URL/YOUTUBE_RTMP_KEY. Put them in $ENV_FILE (gitignored, chmod 600)." >&2
	exit 2
fi
chmod 600 "$ENV_FILE" 2>/dev/null || true

DEV_PID=""
cleanup() {
	set +e
	[[ -n "$DEV_PID" ]] && kill "$DEV_PID" 2>/dev/null || true
	for p in 3000 5173 8787; do
		lsof -tiTCP:"$p" -sTCP:LISTEN 2>/dev/null | xargs -r kill -9 2>/dev/null || true
	done
}
trap cleanup EXIT INT TERM

for p in 3000 5173 8787; do
	lsof -tiTCP:"$p" -sTCP:LISTEN 2>/dev/null | xargs -r kill -9 2>/dev/null || true
done

"$REPO_ROOT/scripts/check-gpu-zero-spend.sh"

BRIVVA_VIDEO_ENCODER=nvenc \
	INFISICAL_ENV="$INFISICAL_ENV" \
	DEV_AUTH_BYPASS=true \
	BOOT_TIMEOUT="$BOOT_TIMEOUT" \
	"$REPO_ROOT/scripts/dev-all.sh" >"$LOG_DIR/local-nvenc-youtube-dev-all.log" 2>&1 &
DEV_PID=$!

for _ in $(seq 1 "$BOOT_TIMEOUT"); do
	if curl -sf http://localhost:3000/health >/dev/null 2>&1 &&
		curl -sf http://localhost:8787/ >/dev/null 2>&1 &&
		curl -sf http://localhost:5173/ >/dev/null 2>&1; then
		break
	fi
	if ! kill -0 "$DEV_PID" 2>/dev/null; then
		echo "dev-all exited early; tail log:" >&2
		tail -160 "$LOG_DIR/local-nvenc-youtube-dev-all.log" >&2
		exit 4
	fi
	sleep 1
done

curl -sf http://localhost:3000/health >/dev/null
curl -sf http://localhost:8787/ >/dev/null
curl -sf http://localhost:5173/ >/dev/null
"$REPO_ROOT/scripts/smoke-media-engine.sh" http://127.0.0.1:3000

BRIVVA_DEV_FRONTEND_URL=http://localhost:5173 \
	BRIVVA_DEV_WORKERS_URL=http://localhost:8787 \
	"$REPO_ROOT/scripts/test-e2e-real.sh" --no-clone --source "$SOURCE_LANG" --youtube "$DEST_LANG" --duration "$DURATION_SECONDS" |
	tee "$LOG_DIR/local-nvenc-youtube-e2e.log"

SESSION_ID="$(cd "$REPO_ROOT/workers" && bun wrangler d1 execute brivva --local --command \
	"SELECT id FROM sessions WHERE user_id='dev-user' AND title LIKE 'e2e:%' ORDER BY created_at DESC LIMIT 1;" |
	node -e '
let text=""; process.stdin.on("data", d => text += d); process.stdin.on("end", () => {
  const id = text.match(/"id"\s*:\s*"([^"]+)"/)?.[1];
  if (id) { console.log(id); return; }
  process.exit(1);
});')"

if [[ -z "$SESSION_ID" ]]; then
	echo "Could not discover latest local e2e session id" >&2
	exit 5
fi

echo "session_id=$SESSION_ID"
BRIVVA_ENGINE_LOG="$LOG_DIR/server-rs.log" "$REPO_ROOT/scripts/verify-session-burn-in.sh" "$SESSION_ID" --local |
	tee "$LOG_DIR/local-nvenc-youtube-verify.txt"
BRIVVA_ENGINE_LOG="$LOG_DIR/server-rs.log" "$REPO_ROOT/scripts/collect-rehearsal-proof.sh" "$SESSION_ID" --local |
	tee "$LOG_DIR/local-nvenc-youtube-proof.txt"

echo "✓ local NVENC YouTube burn-in complete"
echo "session_id=$SESSION_ID"
echo "proof_archive=$LOG_DIR/rehearsals/$SESSION_ID.tar.gz"
