#!/usr/bin/env bash
# Prove Brivva V2 Phase 1 timeline shadow mode with fake browser media.
#
# What it automates:
#   1. Runs a real-stack Playwright rehearsal with Chromium fake camera/mic and
#      BRIVVA_V2_TIMELINE_SHADOW=1.
#   2. Verifies server logs contain video RTP PTS/jitter shadow payloads and
#      non-authoritative audio arrival timing payloads.
#   3. Copies shadow-on logs into a timestamped proof directory.
#   4. Runs a rollback smoke with BRIVVA_V2_TIMELINE_SHADOW=0.
#   5. Copies rollback logs into the proof directory and verifies shadow logs disappear.
#
# Default runtime is intentionally the requested first proof length: 10 minutes.
# Pass destination/source flags through to scripts/test-e2e-auto.sh as needed.
#
# Examples:
#   ./scripts/prove-v2-timeline-shadow.sh
#   ./scripts/prove-v2-timeline-shadow.sh --duration 900 --source ko --youtube ko
#   ./scripts/prove-v2-timeline-shadow.sh --headed --duration 600

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
LOG_DIR="$ROOT/.dev-logs"
SERVER_LOG="$LOG_DIR/server-rs.log"
STACK_LOG="$LOG_DIR/e2e-auto-dev-stack.log"
PROOF_ROOT="${PROOF_ROOT:-$LOG_DIR/v2-timeline-shadow-proof}"
PROOF_DIR="${PROOF_DIR:-$PROOF_ROOT/$(date +%Y%m%d-%H%M%S)}"

SHADOW_SECONDS="${SHADOW_SECONDS:-600}"
ROLLBACK_SECONDS="${ROLLBACK_SECONDS:-45}"
USER_ARGS=()

usage() {
	cat <<'USAGE'
Usage:
  ./scripts/prove-v2-timeline-shadow.sh [flags passed to test-e2e-auto.sh]

Harness flags:
  --duration <sec>            Shadow-on proof duration. Default: 600.
  --rollback-duration <sec>   Flag-off rollback smoke duration. Default: 45.
  -h, --help                  Show help.

Other flags are forwarded to scripts/test-e2e-auto.sh, e.g.:
  --source ko --youtube ko --headed --no-clone

Notes:
  - Uses Playwright Chromium fake media via scripts/test-e2e-auto.sh.
  - Brave/manual fake-media smoke remains available via scripts/dev-fake-media.sh.
  - Requires Docker for the default local RTMP sink and Infisical for dev-all.sh.
USAGE
}

while [[ $# -gt 0 ]]; do
	case "$1" in
	--duration)
		SHADOW_SECONDS="$2"
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
need cargo
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
	shadow-on)
		cp "$SERVER_LOG" "$PROOF_DIR/shadow-on-server-rs.log"
		cp "$STACK_LOG" "$PROOF_DIR/shadow-on-dev-stack.log"
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
	if [[ ${#files[@]} -eq 0 ]]; then
		echo "ERROR: no files provided for $label check" >&2
		exit 1
	fi
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
	if [[ ${#files[@]} -eq 0 ]]; then
		echo "ERROR: no files provided for $label rejection" >&2
		exit 1
	fi
	if grep -F "$pattern" "${files[@]}" >/dev/null 2>&1; then
		echo "ERROR: rollback still emitted $label log pattern: $pattern" >&2
		echo "Checked:" >&2
		printf '  %s\n' "${files[@]}" >&2
		exit 1
	fi
	echo "✓ rollback absent: $label ($pattern)"
}

common_args=(--duration "$SHADOW_SECONDS")
if [[ ${#USER_ARGS[@]} -gt 0 ]]; then
	common_args+=("${USER_ARGS[@]}")
fi

cd "$ROOT"

echo "==> Phase 1 shadow proof: BRIVVA_V2_TIMELINE_SHADOW=1, duration=${SHADOW_SECONDS}s"
BRIVVA_V2_TIMELINE_SHADOW=1 \
	"$ROOT/scripts/test-e2e-auto.sh" "${common_args[@]}"

snapshot_logs "shadow-on"
shadow_logs=(
	"$PROOF_DIR/shadow-on-server-rs.log"
	"$PROOF_DIR/shadow-on-dev-stack.log"
)
require_log_pattern "v2 timeline shadow video" "video shadow event" "${shadow_logs[@]}"
require_log_pattern "video_rtp_authoritative" "video RTP PTS/jitter payload" "${shadow_logs[@]}"
require_log_pattern "v2 timeline shadow audio" "audio shadow event" "${shadow_logs[@]}"
require_log_pattern "audio_arrival_non_authoritative" "non-authoritative audio payload" "${shadow_logs[@]}"

rollback_args=(--duration "$ROLLBACK_SECONDS")
if [[ ${#USER_ARGS[@]} -gt 0 ]]; then
	rollback_args+=("${USER_ARGS[@]}")
fi

echo "==> Rollback proof: BRIVVA_V2_TIMELINE_SHADOW=0, duration=${ROLLBACK_SECONDS}s"
BRIVVA_V2_TIMELINE_SHADOW=0 \
	"$ROOT/scripts/test-e2e-auto.sh" "${rollback_args[@]}"

snapshot_logs "rollback"
rollback_logs=(
	"$PROOF_DIR/rollback-server-rs.log"
	"$PROOF_DIR/rollback-dev-stack.log"
)
reject_log_pattern "v2 timeline shadow video" "video shadow event" "${rollback_logs[@]}"
reject_log_pattern "video_rtp_authoritative" "video RTP PTS/jitter payload" "${rollback_logs[@]}"
reject_log_pattern "v2 timeline shadow audio" "audio shadow event" "${rollback_logs[@]}"
reject_log_pattern "audio_arrival_non_authoritative" "non-authoritative audio payload" "${rollback_logs[@]}"

cat >"$PROOF_DIR/summary.txt" <<SUMMARY
V2 timeline shadow proof complete
shadow_seconds=$SHADOW_SECONDS
rollback_seconds=$ROLLBACK_SECONDS
shadow_flag_on_patterns=video/audio observed
shadow_flag_off_patterns=absent
rollback=disable BRIVVA_V2_TIMELINE_SHADOW
SUMMARY

echo "==> V2 timeline shadow runtime proof complete"
echo "==> Proof logs: $PROOF_DIR"
