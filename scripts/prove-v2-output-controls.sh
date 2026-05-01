#!/usr/bin/env bash
# Prove Brivva V2 Phase 3 output health/control logs with fake browser media.
#
# What it automates:
#   1. Runs a real-stack Playwright rehearsal with BRIVVA_V2_OUTPUT_CONTROLS=1.
#   2. Copies output-controls-on logs into a proof directory.
#   3. Verifies logs-first output health events and stable output_id fields exist.
#   4. Runs a rollback smoke with BRIVVA_V2_OUTPUT_CONTROLS=0.
#   5. Verifies output-control logs disappear when the flag is off.
#
# This is Phase 3 contract proof only. It does not send operator commands and
# does not force/kill/restart FFmpeg beyond the existing RTMP lifecycle.

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
LOG_DIR="$ROOT/.dev-logs"
SERVER_LOG="$LOG_DIR/server-rs.log"
STACK_LOG="$LOG_DIR/e2e-auto-dev-stack.log"
PROOF_ROOT="${PROOF_ROOT:-$LOG_DIR/v2-output-controls-proof}"
PROOF_DIR="${PROOF_DIR:-$PROOF_ROOT/$(date +%Y%m%d-%H%M%S)}"

OUTPUT_SECONDS="${OUTPUT_SECONDS:-90}"
ROLLBACK_SECONDS="${ROLLBACK_SECONDS:-45}"
USER_ARGS=()

usage() {
	cat <<'USAGE'
Usage:
  ./scripts/prove-v2-output-controls.sh [flags passed to test-e2e-auto.sh]

Harness flags:
  --duration <sec>            Output-controls-on proof duration. Default: 90.
  --rollback-duration <sec>   Flag-off rollback smoke duration. Default: 45.
  -h, --help                  Show help.

Other flags are forwarded to scripts/test-e2e-auto.sh, e.g.:
  --source ko --youtube ko --rtmp2 ja --headed --no-clone

Notes:
  - Uses Playwright Chromium fake media via scripts/test-e2e-auto.sh.
  - Default test-e2e-auto path starts a local RTMP sink and one YouTube-shaped output.
  - Requires Docker for the default local RTMP sink and Infisical for dev-all.sh.
USAGE
}

while [[ $# -gt 0 ]]; do
	case "$1" in
	--duration)
		OUTPUT_SECONDS="$2"
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
	output-controls-on)
		cp "$SERVER_LOG" "$PROOF_DIR/output-controls-on-server-rs.log"
		cp "$STACK_LOG" "$PROOF_DIR/output-controls-on-dev-stack.log"
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

proof_args=(--duration "$OUTPUT_SECONDS")
rollback_args=(--duration "$ROLLBACK_SECONDS")
if [[ ${#USER_ARGS[@]} -gt 0 ]]; then
	proof_args+=("${USER_ARGS[@]}")
	rollback_args+=("${USER_ARGS[@]}")
fi

cd "$ROOT"

echo "==> Phase 3 output-controls proof: BRIVVA_V2_OUTPUT_CONTROLS=1, duration=${OUTPUT_SECONDS}s"
BRIVVA_V2_OUTPUT_CONTROLS=1 \
	"$ROOT/scripts/test-e2e-auto.sh" "${proof_args[@]}"

snapshot_logs "output-controls-on"
output_logs=(
	"$PROOF_DIR/output-controls-on-server-rs.log"
	"$PROOF_DIR/output-controls-on-dev-stack.log"
)
require_log_pattern "v2 output health event" "output health log event" "${output_logs[@]}"
require_log_pattern "output.starting" "output.starting event" "${output_logs[@]}"
require_log_pattern "output.live" "output.live event" "${output_logs[@]}"
require_log_pattern "output_id" "stable output_id field" "${output_logs[@]}"

echo "==> Rollback proof: BRIVVA_V2_OUTPUT_CONTROLS=0, duration=${ROLLBACK_SECONDS}s"
BRIVVA_V2_OUTPUT_CONTROLS=0 \
	"$ROOT/scripts/test-e2e-auto.sh" "${rollback_args[@]}"

snapshot_logs "rollback"
rollback_logs=(
	"$PROOF_DIR/rollback-server-rs.log"
	"$PROOF_DIR/rollback-dev-stack.log"
)
reject_log_pattern "v2 output health event" "output health event" "${rollback_logs[@]}"
reject_log_pattern "output.starting" "output.starting event" "${rollback_logs[@]}"
reject_log_pattern "output.live" "output.live event" "${rollback_logs[@]}"

cat >"$PROOF_DIR/summary.txt" <<SUMMARY
V2 output controls proof complete
output_seconds=$OUTPUT_SECONDS
rollback_seconds=$ROLLBACK_SECONDS
output_controls_on=v2 output health event, output.starting, output.live, output_id observed
rollback=disable BRIVVA_V2_OUTPUT_CONTROLS; output-control logs absent with flag off
notes=logs/contracts only; no D1/UI; no new FFmpeg process control
SUMMARY

echo "==> V2 output controls runtime proof complete"
echo "==> Proof logs: $PROOF_DIR"
