#!/usr/bin/env bash
# Verify Brivva FFmpeg runtime invariants for CPU/GPU media engines.

set -euo pipefail

MODE="${1:-${BRIVVA_VIDEO_ENCODER:-x264}}"
FFMPEG_BIN="${FFMPEG_BIN:-ffmpeg}"

fail() {
	echo "✗ $*" >&2
	exit 2
}
pass() { echo "✓ $*"; }

command -v "$FFMPEG_BIN" >/dev/null 2>&1 || fail "$FFMPEG_BIN not found"

"$FFMPEG_BIN" -version | grep -q "enable-openssl" ||
	fail "ffmpeg missing --enable-openssl; Grip/IVS RTMPS unsafe"
pass "OpenSSL enabled"

"$FFMPEG_BIN" -hide_banner -protocols 2>&1 | grep -q "rtmps" ||
	fail "ffmpeg missing rtmps protocol"
pass "rtmps protocol present"

"$FFMPEG_BIN" -hide_banner -filters 2>&1 | grep -q "drawtext" ||
	fail "ffmpeg missing drawtext filter"
pass "drawtext filter present"

if [ "$MODE" = "nvenc" ] || [ "$MODE" = "h264_nvenc" ] || [ "$MODE" = "gpu" ]; then
	"$FFMPEG_BIN" -hide_banner -encoders 2>&1 | grep -q "h264_nvenc" ||
		fail "ffmpeg missing h264_nvenc encoder"
	pass "h264_nvenc encoder present"

	if command -v nvidia-smi >/dev/null 2>&1; then
		nvidia-smi --query-gpu=name,driver_version --format=csv,noheader | sed 's/^/GPU: /'
		pass "nvidia-smi visible"
	else
		echo "⚠ nvidia-smi not found; container build may pass, runtime encode may fail without NVIDIA host driver" >&2
	fi
fi
