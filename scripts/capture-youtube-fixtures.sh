#!/usr/bin/env bash
# §0.5.1 — capture real YouTube Live API responses into
# workers/tests/fixtures/youtube/. Replaces the three
# HAND_CRAFTED_PENDING_REAL_CAPTURE rows currently in the README status
# table.
#
# Prereqs:
#   - Test Google account with YouTube Live Streaming enabled (NOT prod).
#   - OAuth access token with scope
#     https://www.googleapis.com/auth/youtube.
#   - `jq` for pretty-printing.
#
# Usage:
#   export YT_ACCESS_TOKEN=ya29.XXXX
#   bash scripts/capture-youtube-fixtures.sh
#
# Output:
#   workers/tests/fixtures/youtube/broadcast_insert_happy.json
#   workers/tests/fixtures/youtube/stream_insert_happy.json
#   workers/tests/fixtures/youtube/bind_happy.json
#   workers/tests/fixtures/youtube/broadcast_403_quota.json   (if triggered)
#
# After capture, update the README status table's Status column from
# HAND_CRAFTED_PENDING_REAL_CAPTURE → CAPTURED and commit.

set -euo pipefail

: "${YT_ACCESS_TOKEN:?export YT_ACCESS_TOKEN=... first}"

if ! command -v jq >/dev/null 2>&1; then
    echo "error: jq required" >&2
    exit 1
fi

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DEST="${REPO_ROOT}/workers/tests/fixtures/youtube"
mkdir -p "$DEST"

START="$(date -u -v+5M +%Y-%m-%dT%H:%M:%SZ 2>/dev/null || date -u -d '+5 minutes' +%Y-%m-%dT%H:%M:%SZ)"
TITLE="Brivva §0.5.1 capture $(date +%H%M%S)"

echo "── liveBroadcasts.insert ──"
curl -sSfL -X POST \
    "https://www.googleapis.com/youtube/v3/liveBroadcasts?part=snippet,contentDetails,status" \
    -H "Authorization: Bearer ${YT_ACCESS_TOKEN}" \
    -H "Content-Type: application/json" \
    -d "$(jq -n --arg t "$TITLE" --arg s "$START" '{
        snippet: { title: $t, scheduledStartTime: $s },
        status: { privacyStatus: "unlisted", selfDeclaredMadeForKids: false },
        contentDetails: { enableAutoStart: false, enableAutoStop: false }
    }')" |
    jq . > "${DEST}/broadcast_insert_happy.json"
BROADCAST_ID="$(jq -r .id "${DEST}/broadcast_insert_happy.json")"
echo "  broadcast id: ${BROADCAST_ID}"

echo "── liveStreams.insert ──"
curl -sSfL -X POST \
    "https://www.googleapis.com/youtube/v3/liveStreams?part=snippet,cdn,contentDetails,status" \
    -H "Authorization: Bearer ${YT_ACCESS_TOKEN}" \
    -H "Content-Type: application/json" \
    -d "$(jq -n --arg t "$TITLE" '{
        snippet: { title: $t },
        cdn: { frameRate: "30fps", ingestionType: "rtmp", resolution: "1080p" }
    }')" |
    jq . > "${DEST}/stream_insert_happy.json"
STREAM_ID="$(jq -r .id "${DEST}/stream_insert_happy.json")"
echo "  stream id: ${STREAM_ID}"

echo "── liveBroadcasts.bind ──"
curl -sSfL -X POST \
    "https://www.googleapis.com/youtube/v3/liveBroadcasts/bind?part=id,snippet,contentDetails,status&id=${BROADCAST_ID}&streamId=${STREAM_ID}" \
    -H "Authorization: Bearer ${YT_ACCESS_TOKEN}" |
    jq . > "${DEST}/bind_happy.json"

echo
echo "── 403 quota (optional) ──"
echo "To capture broadcast_403_quota.json, intentionally burn through the"
echo "daily quota OR use a fresh project with quota=0. Capture one rejection"
echo "response body, save to ${DEST}/broadcast_403_quota.json."
echo
echo "── cleanup ──"
echo "Delete the capture broadcast when done:"
echo "  curl -X DELETE -H \"Authorization: Bearer \$YT_ACCESS_TOKEN\" \\"
echo "    \"https://www.googleapis.com/youtube/v3/liveBroadcasts?id=${BROADCAST_ID}\""
echo
echo "Now update the Status column in"
echo "  workers/tests/fixtures/youtube/README.md"
echo "from HAND_CRAFTED_PENDING_REAL_CAPTURE → CAPTURED and commit."
