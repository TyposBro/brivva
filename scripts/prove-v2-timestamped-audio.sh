#!/usr/bin/env bash
# Prove Brivva V2 Phase 2 timestamped WS PCM bridge with fake browser media.
#
# What it automates:
#   1. Runs a real-stack Playwright rehearsal with Chromium fake camera/mic,
#      Phase 1 timeline shadow logs enabled, and Phase 2 timestamped audio
#      enabled on both server + frontend.
#   2. Copies timestamped-on logs into a timestamped proof directory.
#   3. Verifies audio timing logs are bridge-derived, not arrival-only.
#   4. Runs raw-PCM compatibility with server flag on and frontend flag off.
#   5. Runs rollback smoke with both timestamped flags off while keeping
#      timeline shadow on.
#   6. Copies phase logs and verifies timestamped/arrival-only labels.

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
LOG_DIR="$ROOT/.dev-logs"
SERVER_LOG="$LOG_DIR/server-rs.log"
STACK_LOG="$LOG_DIR/e2e-auto-dev-stack.log"
PROOF_ROOT="${PROOF_ROOT:-$LOG_DIR/v2-timestamped-audio-proof}"
PROOF_DIR="${PROOF_DIR:-$PROOF_ROOT/$(date +%Y%m%d-%H%M%S)}"

TIMESTAMPED_SECONDS="${TIMESTAMPED_SECONDS:-120}"
ROLLBACK_SECONDS="${ROLLBACK_SECONDS:-45}"
USER_ARGS=()

usage() {
	cat <<'USAGE'
Usage:
  ./scripts/prove-v2-timestamped-audio.sh [flags passed to test-e2e-auto.sh]

Harness flags:
  --duration <sec>            Timestamped-on proof duration. Default: 120.
  --rollback-duration <sec>   Flag-off rollback smoke duration. Default: 45.
  -h, --help                  Show help.

Other flags are forwarded to scripts/test-e2e-auto.sh, e.g.:
  --source ko --youtube ko --headed --no-clone

Notes:
  - Uses Playwright Chromium fake media via scripts/test-e2e-auto.sh.
  - Keeps BRIVVA_V2_TIMELINE_SHADOW=1 for both phases so audio logs are observable.
  - Requires Docker for the default local RTMP sink and Infisical for dev-all.sh.
USAGE
}

while [[ $# -gt 0 ]]; do
	case "$1" in
	--duration)
		TIMESTAMPED_SECONDS="$2"
		shift 2
		;;
	--rollback-duration)
		ROLLBACK_SECONDS="$2"
		shift 2
		;;
	-h | --help)
		usage
		exit 0
		;;
	*)
		USER_ARGS+=("$1")
		shift
		;;
	esac
done

need() {
	if ! command -v "$1" >/dev/null 2>&1; then
		echo "ERROR: missing required command: $1" >&2
		exit 1
	fi
}
need docker
need infisical
need bun
mkdir -p "$PROOF_DIR"

require_source_logs() {
	if [[ ! -f "$SERVER_LOG" || ! -f "$STACK_LOG" ]]; then
		echo "ERROR: expected logs missing under $LOG_DIR" >&2
		echo "  $SERVER_LOG" >&2
		echo "  $STACK_LOG" >&2
		exit 1
	fi
}

snapshot_logs() {
	local phase="$1"
	require_source_logs
	case "$phase" in
	timestamped-on)
		cp "$SERVER_LOG" "$PROOF_DIR/timestamped-on-server-rs.log"
		cp "$STACK_LOG" "$PROOF_DIR/timestamped-on-dev-stack.log"
		;;
	server-on-frontend-off)
		cp "$SERVER_LOG" "$PROOF_DIR/server-on-frontend-off-server-rs.log"
		cp "$STACK_LOG" "$PROOF_DIR/server-on-frontend-off-dev-stack.log"
		;;
	rollback)
		cp "$SERVER_LOG" "$PROOF_DIR/rollback-server-rs.log"
		cp "$STACK_LOG" "$PROOF_DIR/rollback-dev-stack.log"
		;;
	*)
		echo "ERROR: unknown log snapshot phase: $phase" >&2
		exit 1
		;;
	esac
}

require_log_pattern() {
	local pattern="$1"
	local label="$2"
	shift 2
	local files=("$@")
	if ! grep -F "$pattern" "${files[@]}" >/dev/null 2>&1; then
		echo "ERROR: missing expected $label log pattern: $pattern" >&2
		echo "Checked:" >&2
		printf '  %s\n' "${files[@]}" >&2
		exit 1
	fi
	echo "✓ observed $label logs ($pattern)"
}

reject_log_pattern() {
	local pattern="$1"
	local label="$2"
	shift 2
	local files=("$@")
	if grep -F "$pattern" "${files[@]}" >/dev/null 2>&1; then
		echo "ERROR: rollback still emitted $label log pattern: $pattern" >&2
		echo "Checked:" >&2
		printf '  %s\n' "${files[@]}" >&2
		exit 1
	fi
	echo "✓ rollback absent: $label ($pattern)"
}

proof_args=(--duration "$TIMESTAMPED_SECONDS")
rollback_args=(--duration "$ROLLBACK_SECONDS")
if [[ ${#USER_ARGS[@]} -gt 0 ]]; then
	proof_args+=("${USER_ARGS[@]}")
	rollback_args+=("${USER_ARGS[@]}")
fi

cd "$ROOT"

echo "==> Phase 2 timestamped audio proof: duration=${TIMESTAMPED_SECONDS}s"
BRIVVA_V2_TIMELINE_SHADOW=1 \
	BRIVVA_V2_TIMESTAMPED_AUDIO=1 \
	VITE_BRIVVA_V2_TIMESTAMPED_AUDIO=1 \
	"$ROOT/scripts/test-e2e-auto.sh" "${proof_args[@]}"

snapshot_logs "timestamped-on"
timestamped_logs=(
	"$PROOF_DIR/timestamped-on-server-rs.log"
	"$PROOF_DIR/timestamped-on-dev-stack.log"
)
require_log_pattern "v2 timeline shadow audio" "audio shadow event" "${timestamped_logs[@]}"
require_log_pattern "audio_timestamped_pcm_bridge_derived" "timestamped bridge-derived audio payload" "${timestamped_logs[@]}"
require_log_pattern "media_pts_us" "audio media PTS field" "${timestamped_logs[@]}"
reject_log_pattern "audio_arrival_non_authoritative" "arrival-only audio payload during timestamped proof" "${timestamped_logs[@]}"

echo "==> Raw PCM compatibility: server timestamped flag on, frontend flag off, duration=${ROLLBACK_SECONDS}s"
BRIVVA_V2_TIMELINE_SHADOW=1 \
	BRIVVA_V2_TIMESTAMPED_AUDIO=1 \
	VITE_BRIVVA_V2_TIMESTAMPED_AUDIO=0 \
	"$ROOT/scripts/test-e2e-auto.sh" "${rollback_args[@]}"

snapshot_logs "server-on-frontend-off"
server_on_raw_logs=(
	"$PROOF_DIR/server-on-frontend-off-server-rs.log"
	"$PROOF_DIR/server-on-frontend-off-dev-stack.log"
)
require_log_pattern "v2 timeline shadow audio" "server-on/frontend-off audio shadow event" "${server_on_raw_logs[@]}"
require_log_pattern "audio_arrival_non_authoritative" "server-on/frontend-off raw PCM arrival payload" "${server_on_raw_logs[@]}"
reject_log_pattern "audio_timestamped_pcm_bridge_derived" "server-on/frontend-off timestamped bridge payload" "${server_on_raw_logs[@]}"

echo "==> Rollback proof: both timestamped flags disabled, duration=${ROLLBACK_SECONDS}s"
BRIVVA_V2_TIMELINE_SHADOW=1 \
	BRIVVA_V2_TIMESTAMPED_AUDIO=0 \
	VITE_BRIVVA_V2_TIMESTAMPED_AUDIO=0 \
	"$ROOT/scripts/test-e2e-auto.sh" "${rollback_args[@]}"

snapshot_logs "rollback"
rollback_logs=(
	"$PROOF_DIR/rollback-server-rs.log"
	"$PROOF_DIR/rollback-dev-stack.log"
)
require_log_pattern "v2 timeline shadow audio" "rollback audio shadow event" "${rollback_logs[@]}"
require_log_pattern "audio_arrival_non_authoritative" "rollback arrival-only audio payload" "${rollback_logs[@]}"
reject_log_pattern "audio_timestamped_pcm_bridge_derived" "timestamped bridge-derived audio payload" "${rollback_logs[@]}"

cat >"$PROOF_DIR/summary.txt" <<SUMMARY
V2 timestamped audio proof complete
timestamped_seconds=$TIMESTAMPED_SECONDS
rollback_seconds=$ROLLBACK_SECONDS
timestamped_on=audio_timestamped_pcm_bridge_derived observed
server_on_frontend_off=raw PCM accepted with audio_arrival_non_authoritative
both_flags_off=audio_arrival_non_authoritative observed and bridge-derived absent
rollback=disable VITE_BRIVVA_V2_TIMESTAMPED_AUDIO first, then BRIVVA_V2_TIMESTAMPED_AUDIO
SUMMARY

echo "==> V2 timestamped audio runtime proof complete"
echo "==> Proof logs: $PROOF_DIR"
