#!/usr/bin/env bash
#
# run-local-engine.sh — production-shaped Brivva media engine on Aziz laptop.
#
# Use for local GPU iteration and May 10 hot fallback. Cloudflare Pages/Workers/D1
# remain remote by default; only server-rs media hot path runs on this machine.
#
# Usage:
#   ./scripts/run-local-engine.sh
#   ./scripts/run-local-engine.sh --preflight-only
#   BRIVVA_VIDEO_ENCODER=x264 ./scripts/run-local-engine.sh
#   WORKERS_API_URL=http://localhost:8787 ./scripts/run-local-engine.sh

set -euo pipefail

ORIGINAL_ARGS=("$@")
PREFLIGHT_ONLY=false

usage() {
	cat <<EOF
Usage: $0 [--preflight-only]

Run production-shaped Brivva media engine on this laptop.

--preflight-only  Check local GPU/FFmpeg/env/ports, then exit before cargo run.
EOF
	exit 1
}

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

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
LOG_DIR="${REPO_ROOT}/.dev-logs"
mkdir -p "$LOG_DIR"

if [ "${BRIVVA_INFISICAL_WRAPPED:-0}" != "1" ]; then
	if command -v infisical >/dev/null 2>&1; then
		export BRIVVA_INFISICAL_WRAPPED=1
		infisical_args=(run --env="${INFISICAL_ENV:-dev}" --path="${INFISICAL_PATH:-/}")
		if [ -f "${REPO_ROOT}/.infisical.json" ]; then
			infisical_args+=(--project-config-dir "$REPO_ROOT")
		fi
		exec infisical "${infisical_args[@]}" -- "$0" "${ORIGINAL_ARGS[@]}"
	fi
	echo "WARN: infisical CLI not found; using current shell env" >&2
fi

SERVER_PORT="${SERVER_PORT:-3000}"
export FRONTEND_URL="${FRONTEND_URL:-https://brivva.pages.dev}"
export WORKERS_API_URL="${WORKERS_API_URL:-https://brivva-api.milliytechnology.workers.dev}"
export BRIVVA_VIDEO_ENCODER="${BRIVVA_VIDEO_ENCODER:-nvenc}"
export BRIVVA_WEBRTC_UDP_PORT_MIN="${BRIVVA_WEBRTC_UDP_PORT_MIN:-40000}"
export BRIVVA_WEBRTC_UDP_PORT_MAX="${BRIVVA_WEBRTC_UDP_PORT_MAX:-40100}"
export BRIVVA_SESSION_LOGS="${BRIVVA_SESSION_LOGS:-1}"

RED=$'\033[0;31m'
GREEN=$'\033[0;32m'
YELLOW=$'\033[0;33m'
DIM=$'\033[2m'
NC=$'\033[0m'
info() { echo "${DIM}$*${NC}"; }
pass() { echo "${GREEN}✓ $*${NC}"; }
warn() { echo "${YELLOW}⚠ $*${NC}"; }
fail() { echo "${RED}✗ $*${NC}"; }

require_cmd() {
	if ! command -v "$1" >/dev/null 2>&1; then
		fail "$1 not found"
		exit 2
	fi
}

require_cmd cargo
require_cmd ffmpeg

if lsof -iTCP:"$SERVER_PORT" -sTCP:LISTEN >/dev/null 2>&1; then
	fail "port $SERVER_PORT already in use"
	lsof -iTCP:"$SERVER_PORT" -sTCP:LISTEN
	exit 2
fi
pass "port $SERVER_PORT free"

ffmpeg_encoders="$(ffmpeg -hide_banner -encoders 2>/dev/null)"
ffmpeg_protocols="$(ffmpeg -hide_banner -protocols 2>&1)"
ffmpeg_filters="$(ffmpeg -hide_banner -filters 2>&1)"

if [ "$BRIVVA_VIDEO_ENCODER" = "nvenc" ] || [ "$BRIVVA_VIDEO_ENCODER" = "h264_nvenc" ]; then
	if ! grep -q "h264_nvenc" <<<"$ffmpeg_encoders"; then
		fail "ffmpeg lacks h264_nvenc encoder; install NVIDIA-enabled ffmpeg or set BRIVVA_VIDEO_ENCODER=x264"
		exit 2
	fi
	pass "ffmpeg h264_nvenc encoder present"
	if command -v nvidia-smi >/dev/null 2>&1; then
		nvidia-smi --query-gpu=name,driver_version --format=csv,noheader | sed 's/^/GPU: /'
		pass "nvidia-smi visible"
	else
		warn "nvidia-smi not found; NVENC may still fail at runtime if NVIDIA driver/runtime missing"
	fi
else
	warn "BRIVVA_VIDEO_ENCODER=$BRIVVA_VIDEO_ENCODER; CPU x264 path selected"
fi

grep -q "rtmps" <<<"$ffmpeg_protocols" || {
	fail "ffmpeg lacks rtmps protocol"
	exit 2
}
grep -q "drawtext" <<<"$ffmpeg_filters" || {
	fail "ffmpeg lacks drawtext filter"
	exit 2
}
pass "ffmpeg has rtmps + drawtext"

warn "Expose UDP ${BRIVVA_WEBRTC_UDP_PORT_MIN}-${BRIVVA_WEBRTC_UDP_PORT_MAX}/udp to this laptop for remote WebRTC video. Cloudflare Tunnel covers HTTPS/WSS, not this UDP media path."
info "WORKERS_API_URL=$WORKERS_API_URL"
info "FRONTEND_URL=$FRONTEND_URL"
info "BRIVVA_VIDEO_ENCODER=$BRIVVA_VIDEO_ENCODER"
info "logs → ${LOG_DIR}/local-engine.log"

if [ "$PREFLIGHT_ONLY" = true ]; then
	pass "local engine preflight complete"
	exit 0
fi

cd "${REPO_ROOT}/server-rs"
exec cargo run 2>&1 | tee "${LOG_DIR}/local-engine.log"
