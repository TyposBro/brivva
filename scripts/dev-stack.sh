#!/usr/bin/env bash
#
# dev-stack.sh — boot server-rs + workers locally, wait until both are
# healthy, run the local smoke test, then tear everything down.
#
# Usage:
#   ./scripts/dev-stack.sh              # boot → smoke → exit
#   ./scripts/dev-stack.sh --keep       # boot → smoke → leave running
#                                        (Ctrl-C to stop)
#
# Environment:
#   SERVER_PORT=3000     server-rs listen port (must match wrangler binding)
#   WORKERS_PORT=8787    wrangler dev port
#   BOOT_TIMEOUT=60      seconds to wait for /health
#   INFISICAL_ENV=dev    Infisical environment to inject at runtime
#   INFISICAL_PATH=/     Infisical secret path to inject at runtime
#
# Exit codes:
#   0 — smoke passed
#   1 — smoke failed
#   2 — services failed to boot within BOOT_TIMEOUT

set -u

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
LOG_DIR="${REPO_ROOT}/.dev-logs"
mkdir -p "$LOG_DIR"

if [ "${BRIVVA_INFISICAL_WRAPPED:-0}" != "1" ]; then
  if ! command -v infisical >/dev/null 2>&1; then
    echo "infisical CLI not found; install/login first" >&2
    exit 2
  fi
  export BRIVVA_INFISICAL_WRAPPED=1
  infisical_args=(run --env="${INFISICAL_ENV:-dev}" --path="${INFISICAL_PATH:-/}")
  if [ -f "${REPO_ROOT}/.infisical.json" ]; then
    infisical_args+=(--project-config-dir "$REPO_ROOT")
  fi
  exec infisical "${infisical_args[@]}" -- "$0" "$@"
fi

SERVER_PORT="${SERVER_PORT:-3000}"
WORKERS_PORT="${WORKERS_PORT:-8787}"
BOOT_TIMEOUT="${BOOT_TIMEOUT:-60}"
FRONTEND_PORT="${FRONTEND_PORT:-5173}"

KEEP=0
[ "${1:-}" = "--keep" ] && KEEP=1

RED=$'\033[0;31m'
GREEN=$'\033[0;32m'
YELLOW=$'\033[0;33m'
DIM=$'\033[2m'
NC=$'\033[0m'

info()  { echo "${DIM}$*${NC}"; }
pass()  { echo "${GREEN}✓ $*${NC}"; }
warn()  { echo "${YELLOW}⚠ $*${NC}"; }
fail()  { echo "${RED}✗ $*${NC}"; }

SERVER_PID=""
WORKERS_PID=""
WORKERS_ENV_FILE=""

cleanup() {
  local rc=$?
  if [ $KEEP -eq 1 ] && [ $rc -eq 0 ]; then
    info "--keep set; leaving services running (server_pid=$SERVER_PID workers_pid=$WORKERS_PID)"
    info "kill them with:  kill $SERVER_PID $WORKERS_PID"
    return
  fi
  info "tearing down dev stack"
  [ -n "$SERVER_PID" ]  && kill "$SERVER_PID"  2>/dev/null || true
  [ -n "$WORKERS_PID" ] && kill "$WORKERS_PID" 2>/dev/null || true
  [ -n "$WORKERS_ENV_FILE" ] && rm -f "$WORKERS_ENV_FILE"
  # Give them a moment to flush logs, then hard-kill any stragglers on our ports.
  sleep 1
  lsof -ti :"$SERVER_PORT"  2>/dev/null | xargs kill -9 2>/dev/null || true
  lsof -ti :"$WORKERS_PORT" 2>/dev/null | xargs kill -9 2>/dev/null || true
}
trap cleanup EXIT INT TERM

# ── Pre-flight: ports free? ──────────────────────────────────
for port in "$SERVER_PORT" "$WORKERS_PORT"; do
  if lsof -iTCP:"$port" -sTCP:LISTEN >/dev/null 2>&1; then
    fail "port $port already in use"
    info "running processes:"
    lsof -iTCP:"$port" -sTCP:LISTEN
    exit 2
  fi
done

# ── Runtime secrets: Infisical env → server-rs env + temp Wrangler env ──
export WORKERS_API_URL="http://localhost:${WORKERS_PORT}"
export FRONTEND_URL="http://localhost:${FRONTEND_PORT}"
export OAUTH_REDIRECT_URI="http://localhost:${WORKERS_PORT}/auth/youtube/callback"
export GOOGLE_SIGNIN_REDIRECT_URI="http://localhost:${WORKERS_PORT}/auth/google/callback"

dotenv_line() {
  local key="$1"
  local value="${!key-}"
  [ -z "$value" ] && return 0
  value="${value//\\/\\\\}"
  value="${value//\"/\\\"}"
  printf '%s="%s"\n' "$key" "$value"
}

WORKERS_ENV_FILE="$(mktemp "${TMPDIR:-/tmp}/brivva-workers.XXXXXX.env")"
chmod 600 "$WORKERS_ENV_FILE"
for key in \
  FRONTEND_URL OAUTH_REDIRECT_URI GOOGLE_SIGNIN_REDIRECT_URI \
  ELEVENLABS_API_KEY GOOGLE_CLIENT_ID GOOGLE_CLIENT_SECRET \
  JWT_SECRET INTERNAL_SECRET GRIP_ACCESS_KEY GRIP_SECRET_KEY STRIPE_WEBHOOK_SECRET
do
  dotenv_line "$key" >> "$WORKERS_ENV_FILE"
done

# ── Boot ─────────────────────────────────────────────────────

info "starting server-rs → ${LOG_DIR}/server-rs.log"
( cd "${REPO_ROOT}/server-rs" && cargo run ) >"${LOG_DIR}/server-rs.log" 2>&1 &
SERVER_PID=$!

info "applying D1 migrations (local) → ${LOG_DIR}/migrations.log"
( cd "${REPO_ROOT}/workers" && bun wrangler d1 migrations apply brivva --local ) \
  >"${LOG_DIR}/migrations.log" 2>&1
if [ $? -ne 0 ]; then
  fail "D1 migrations failed — see ${LOG_DIR}/migrations.log"
  tail -n 30 "${LOG_DIR}/migrations.log" | sed 's/^/  /'
  exit 2
fi
pass "D1 migrations up to date"

info "starting workers → ${LOG_DIR}/workers.log"
( cd "${REPO_ROOT}/workers" && bun wrangler dev --env-file "$WORKERS_ENV_FILE" --port "$WORKERS_PORT" ) \
  >"${LOG_DIR}/workers.log" 2>&1 &
WORKERS_PID=$!

# ── Wait for health ──────────────────────────────────────────
info "waiting up to ${BOOT_TIMEOUT}s for services to come up"
deadline=$(( $(date +%s) + BOOT_TIMEOUT ))
server_ready=0
workers_ready=0
while [ $(date +%s) -lt $deadline ]; do
  if [ $server_ready -eq 0 ]; then
    if curl -sf -m 2 "http://localhost:${SERVER_PORT}/health" >/dev/null 2>&1; then
      pass "server-rs ready on :${SERVER_PORT}"
      server_ready=1
    elif ! kill -0 "$SERVER_PID" 2>/dev/null; then
      fail "server-rs process died — see ${LOG_DIR}/server-rs.log"
      tail -n 30 "${LOG_DIR}/server-rs.log" | sed 's/^/  /'
      exit 2
    fi
  fi
  if [ $workers_ready -eq 0 ]; then
    # wrangler dev has no /health — any TCP answer is enough.
    if curl -s -m 2 -o /dev/null "http://localhost:${WORKERS_PORT}/"; then
      pass "workers ready on :${WORKERS_PORT}"
      workers_ready=1
    elif ! kill -0 "$WORKERS_PID" 2>/dev/null; then
      fail "workers process died — see ${LOG_DIR}/workers.log"
      tail -n 30 "${LOG_DIR}/workers.log" | sed 's/^/  /'
      exit 2
    fi
  fi
  [ $server_ready -eq 1 ] && [ $workers_ready -eq 1 ] && break
  sleep 1
done

if [ $server_ready -ne 1 ] || [ $workers_ready -ne 1 ]; then
  fail "services did not come up within ${BOOT_TIMEOUT}s"
  [ $server_ready -ne 1 ]  && tail -n 30 "${LOG_DIR}/server-rs.log" | sed 's/^/  server-rs > /'
  [ $workers_ready -ne 1 ] && tail -n 30 "${LOG_DIR}/workers.log"   | sed 's/^/  workers   > /'
  exit 2
fi

# ── Smoke ────────────────────────────────────────────────────
echo ""
SERVER_URL="http://localhost:${SERVER_PORT}" \
WORKERS_URL="http://localhost:${WORKERS_PORT}" \
  "${REPO_ROOT}/scripts/smoke-test.sh" local
rc=$?

if [ $rc -ne 0 ]; then
  fail "smoke-test failed (exit $rc)"
  exit $rc
fi

exit 0
