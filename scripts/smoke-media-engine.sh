#!/usr/bin/env bash
set -euo pipefail

MEDIA_URL="${1:-${VITE_MEDIA_URL:-http://127.0.0.1:3000}}"
MEDIA_URL="${MEDIA_URL%/}"

usage() {
	cat <<EOF
Usage: $0 [MEDIA_URL]

Smoke-check a Brivva media engine HTTP endpoint.
Defaults to VITE_MEDIA_URL or http://127.0.0.1:3000.
Checks /health and / banner. WebRTC/session smoke still needs frontend host flow.
EOF
	exit 1
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
	usage
fi

need() {
	if ! command -v "$1" >/dev/null 2>&1; then
		echo "Missing required command: $1" >&2
		exit 1
	fi
}
need curl

health="$(curl -fsS --max-time 5 "$MEDIA_URL/health")"
if [[ "$health" != "ok" ]]; then
	echo "Bad /health response from $MEDIA_URL: $health" >&2
	exit 2
fi

echo "✓ /health ok ($MEDIA_URL)"

banner="$(curl -fsS --max-time 5 "$MEDIA_URL/")"
if [[ "$banner" != *"Brivva Translation Server"* ]]; then
	echo "Bad / response from $MEDIA_URL: $banner" >&2
	exit 3
fi

echo "✓ / banner ok"
echo "media_url=$MEDIA_URL"
echo "ws_url=${MEDIA_URL/http:\/\//ws://}/api/session"
echo "next: point VITE_MEDIA_URL at media_url and run frontend host session"
