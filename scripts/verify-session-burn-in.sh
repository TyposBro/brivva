#!/usr/bin/env bash
# Verify May 10 rehearsal evidence for one Brivva session.
#
# Usage:
#   ./scripts/verify-session-burn-in.sh <session_id> [--remote|--local]
#
# Checks D1 for:
#   - sessions.status + live_session_id
#   - session_metrics row with source/output seconds
#   - session_log_events count + important events
# Also checks local/ECS logs for h264_nvenc evidence when a log file is present.

set -euo pipefail

SESSION_ID="${1:-}"
SCOPE="${2:---remote}"
LOG_FILE="${BRIVVA_ENGINE_LOG:-.dev-logs/local-engine.log}"

if [ -z "$SESSION_ID" ]; then
	echo "usage: $0 <session_id> [--remote|--local]" >&2
	exit 2
fi

case "$SCOPE" in
--remote | --local) ;;
*)
	echo "scope must be --remote or --local" >&2
	exit 2
	;;
esac

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WORKERS_DIR="${REPO_ROOT}/workers"
DB_ARGS=(d1 execute brivva "$SCOPE")

RED=$'\033[0;31m'
GREEN=$'\033[0;32m'
YELLOW=$'\033[0;33m'
NC=$'\033[0m'
pass() { echo "${GREEN}✓ $*${NC}"; }
warn() { echo "${YELLOW}⚠ $*${NC}"; }
fail() {
	echo "${RED}✗ $*${NC}"
	exit 1
}

command -v bun >/dev/null 2>&1 || fail "bun not found"

run_sql() {
	local sql="$1"
	(cd "$WORKERS_DIR" && bun wrangler "${DB_ARGS[@]}" --command "$sql")
}

extract_json_field() {
	local field="$1"
	node -e '
    const fs = require("fs");
    const field = process.argv[1];
    const text = fs.readFileSync(0, "utf8");
    const match = text.match(/\[[\s\S]*\]/);
    if (!match) process.exit(3);
    const payload = JSON.parse(match[0]);
    const row = payload[0]?.results?.[0] || {};
    const value = row[field];
    if (value === undefined || value === null) process.exit(4);
    process.stdout.write(String(value));
  ' "$field"
}

SESSION_SQL="SELECT id,status,COALESCE(live_session_id,'') AS live_session_id FROM sessions WHERE id='$SESSION_ID';"
SESSION_OUT="$(run_sql "$SESSION_SQL")"
echo "$SESSION_OUT"
STATUS="$(printf '%s' "$SESSION_OUT" | extract_json_field status || true)"
[ -n "$STATUS" ] || fail "session row missing: $SESSION_ID"
case "$STATUS" in
live | ended | setup) pass "session row exists status=$STATUS" ;;
*) warn "unexpected session status=$STATUS" ;;
esac

METRICS_SQL="SELECT source_seconds,output_seconds_json,updated_at FROM session_metrics WHERE session_id='$SESSION_ID';"
METRICS_OUT="$(run_sql "$METRICS_SQL")"
echo "$METRICS_OUT"
SOURCE_SECONDS="$(printf '%s' "$METRICS_OUT" | extract_json_field source_seconds || true)"
if [ -n "$SOURCE_SECONDS" ]; then
	node -e 'process.exit(Number(process.argv[1]) > 0 ? 0 : 1)' "$SOURCE_SECONDS" &&
		pass "session_metrics source_seconds=$SOURCE_SECONDS" ||
		fail "session_metrics source_seconds not >0 ($SOURCE_SECONDS)"
else
	fail "session_metrics row missing; run stream for 30s+ before verifying"
fi

LOG_COUNT_SQL="SELECT COUNT(*) AS count FROM session_log_events WHERE session_id='$SESSION_ID';"
LOG_COUNT_OUT="$(run_sql "$LOG_COUNT_SQL")"
echo "$LOG_COUNT_OUT"
LOG_COUNT="$(printf '%s' "$LOG_COUNT_OUT" | extract_json_field count || echo 0)"
if [ "$LOG_COUNT" -gt 0 ]; then
	pass "session_log_events count=$LOG_COUNT"
else
	warn "no session_log_events rows; check SESSION_LOGS_ENABLED/BRIVVA_SESSION_LOGS"
fi

EVENT_SQL="SELECT event,level FROM session_log_events WHERE session_id='$SESSION_ID' AND event IN ('workers.session_created','workers.session_status_updated','server.ws_accepted','server.bootstrap_complete','server.first_audio_received','server.ws_closed') ORDER BY ts_ms;"
run_sql "$EVENT_SQL"

if [ -f "$LOG_FILE" ]; then
	if grep -q "h264_nvenc\|video_encoder.*h264_nvenc" "$LOG_FILE"; then
		pass "NVENC evidence found in $LOG_FILE"
	else
		warn "no NVENC evidence in $LOG_FILE; ensure BRIVVA_VIDEO_ENCODER=nvenc and logs include video_encoder"
	fi
else
	warn "engine log not found: $LOG_FILE (set BRIVVA_ENGINE_LOG for ECS/exported logs)"
fi

pass "burn-in verification complete for $SESSION_ID"
