#!/usr/bin/env bash
#
# dev-all.sh — boot the full local stack for interactive browser testing.
#
# Runs in ONE terminal:
#   - server-rs     on :3000  (Fargate media server / WS audio uplink)
#   - workers       on :8787  (Cloudflare Workers + D1 + OAuth)
#   - vite dev      on :5173  (React frontend)
#
# Then tails all three log files. Ctrl-C kills all three cleanly (EXIT trap).
#
# Usage:
#   ./scripts/dev-all.sh
#
# Env knobs:
#   SERVER_PORT=3000     server-rs port
#   WORKERS_PORT=8787    wrangler dev port
#   FRONTEND_PORT=5173   vite port
#   BOOT_TIMEOUT=60      seconds to wait for server-rs /health + workers /
#   INFISICAL_ENV=dev    Infisical environment to inject at runtime
#   INFISICAL_PATH=/     Infisical secret path to inject at runtime
#
# What this does NOT do:
#   - Run smoke-test (use ./scripts/dev-stack.sh for that; boots server-rs +
#     workers only, runs smoke, tears down).
#   - Proxy browser OAuth callbacks back to localhost. For OAuth to work in
#     local dev you must whitelist these in Google Cloud Console
#     (same OAuth client as prod):
#       http://localhost:8787/auth/google/callback
#       http://localhost:8787/auth/youtube/callback
#     Until then, sign-in / YouTube-connect round-trips will fail at the
#     redirect step. Everything else (voice clone, session create, etc.)
#     works fully local.
#
# Exit codes:
#   0 — clean shutdown via Ctrl-C
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
FRONTEND_PORT="${FRONTEND_PORT:-5173}"
BOOT_TIMEOUT="${BOOT_TIMEOUT:-60}"

RED=$'\033[0;31m'
GREEN=$'\033[0;32m'
YELLOW=$'\033[0;33m'
BLUE=$'\033[0;34m'
DIM=$'\033[2m'
NC=$'\033[0m'

info()  { echo "${DIM}$*${NC}"; }
pass()  { echo "${GREEN}✓ $*${NC}"; }
warn()  { echo "${YELLOW}⚠ $*${NC}"; }
fail()  { echo "${RED}✗ $*${NC}"; }

SERVER_PID=""
WORKERS_PID=""
FRONTEND_PID=""
WORKERS_ENV_FILE=""

cleanup() {
  info ""
  info "tearing down dev stack (Ctrl-C received)"
  [ -n "$SERVER_PID" ]   && kill "$SERVER_PID"   2>/dev/null || true
  [ -n "$WORKERS_PID" ]  && kill "$WORKERS_PID"  2>/dev/null || true
  [ -n "$FRONTEND_PID" ] && kill "$FRONTEND_PID" 2>/dev/null || true
  [ -n "$WORKERS_ENV_FILE" ] && rm -f "$WORKERS_ENV_FILE"
  sleep 1
  for port in "$SERVER_PORT" "$WORKERS_PORT" "$FRONTEND_PORT"; do
    lsof -ti :"$port" 2>/dev/null | xargs kill -9 2>/dev/null || true
  done
}
trap cleanup EXIT INT TERM

# ── Pre-flight: ports free? ──────────────────────────────────
for port in "$SERVER_PORT" "$WORKERS_PORT" "$FRONTEND_PORT"; do
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
  JWT_SECRET INTERNAL_SECRET GRIP_ACCESS_KEY GRIP_SECRET_KEY STRIPE_WEBHOOK_SECRET \
  DEV_AUTH_BYPASS
do
  dotenv_line "$key" >> "$WORKERS_ENV_FILE"
done

# ── Boot server-rs ──────────────────────────────────────────
info "[server-rs] starting → ${LOG_DIR}/server-rs.log"
( cd "${REPO_ROOT}/server-rs" && cargo run ) >"${LOG_DIR}/server-rs.log" 2>&1 &
SERVER_PID=$!

# ── Apply D1 migrations to the local sqlite file ───────────
# `wrangler dev` creates an empty SQLite under .wrangler/state/ but does
# NOT auto-apply migrations — every table query 500s until we do. Run
# `migrations apply --local` synchronously BEFORE starting the worker so
# it's ready on first boot. Subsequent runs are idempotent (wrangler
# tracks applied migrations in `d1_migrations`).
info "[workers] applying D1 migrations (local)…"
( cd "${REPO_ROOT}/workers" && bun wrangler d1 migrations apply brivva --local ) \
  >"${LOG_DIR}/migrations.log" 2>&1
if [ $? -ne 0 ]; then
  fail "D1 migrations failed — see ${LOG_DIR}/migrations.log"
  tail -n 30 "${LOG_DIR}/migrations.log" | sed 's/^/  /'
  exit 2
fi
pass "D1 migrations up to date"

# ── Boot workers ────────────────────────────────────────────
info "[workers] starting → ${LOG_DIR}/workers.log"
( cd "${REPO_ROOT}/workers" && bun wrangler dev --env-file "$WORKERS_ENV_FILE" --port "$WORKERS_PORT" ) \
  >"${LOG_DIR}/workers.log" 2>&1 &
WORKERS_PID=$!

# ── Wait for server-rs + workers health ─────────────────────
info "waiting up to ${BOOT_TIMEOUT}s for server-rs + workers to come up"
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
  exit 2
fi

# ── Boot vite ───────────────────────────────────────────────
info "[frontend] starting → ${LOG_DIR}/frontend.log"
( cd "${REPO_ROOT}/frontend" && \
  VITE_API_URL="http://localhost:${WORKERS_PORT}" \
  VITE_WORKER_URL="http://localhost:${SERVER_PORT}" \
  bun run dev -- --port "$FRONTEND_PORT" --strictPort ) \
  >"${LOG_DIR}/frontend.log" 2>&1 &
FRONTEND_PID=$!

# Vite starts fast; poll briefly.
sleep 2
for i in 1 2 3 4 5 6; do
  if curl -s -m 2 -o /dev/null "http://localhost:${FRONTEND_PORT}/"; then
    pass "frontend ready on :${FRONTEND_PORT}"
    break
  fi
  sleep 1
done

# ── Done — tail all three ───────────────────────────────────
echo ""
echo "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo "${BLUE}  Open:${NC}  http://localhost:${FRONTEND_PORT}"
echo "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo ""
echo "${DIM}Tailing logs. Ctrl-C stops everything.${NC}"
echo ""

# Tail all three logs with prefixes.
tail -F -q \
  "${LOG_DIR}/server-rs.log" \
  "${LOG_DIR}/workers.log" \
  "${LOG_DIR}/frontend.log" 2>/dev/null &
TAIL_PID=$!

# Block until Ctrl-C. Poll every second so a dead child is noticed within 1s
# (we print a warning + keep going so the other two can still be inspected).
# bash 3.2 has no `wait -n`, so we poll kill -0 instead.
server_alive=1
workers_alive=1
frontend_alive=1
while true; do
  if [ $server_alive -eq 1 ] && ! kill -0 "$SERVER_PID" 2>/dev/null; then
    warn "server-rs died (tail .dev-logs/server-rs.log)"
    server_alive=0
  fi
  if [ $workers_alive -eq 1 ] && ! kill -0 "$WORKERS_PID" 2>/dev/null; then
    warn "workers died (tail .dev-logs/workers.log)"
    workers_alive=0
  fi
  if [ $frontend_alive -eq 1 ] && ! kill -0 "$FRONTEND_PID" 2>/dev/null; then
    warn "frontend died (tail .dev-logs/frontend.log)"
    frontend_alive=0
  fi
  # If all three died, nothing to keep running for — let cleanup close out.
  if [ $server_alive -eq 0 ] && [ $workers_alive -eq 0 ] && [ $frontend_alive -eq 0 ]; then
    fail "all services exited; tearing down"
    break
  fi
  sleep 1
done

exit 0
