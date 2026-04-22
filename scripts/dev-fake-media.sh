#!/usr/bin/env bash
# Launch Chrome against the local dev stack with a pre-recorded video
# file standing in for the host's camera + mic. Frontend + backend see
# a normal getUserMedia stream — indistinguishable from real hardware —
# so the full pipeline (Soniox, translation, ElevenLabs, ffmpeg, Grip)
# runs end to end without a human reading a script into a microphone.
#
# Fixtures live in tests/e2e/fixtures/ (gitignored; regenerate from any
# source MP4 with the ffmpeg commands in that README).
#
# Requires: the dev stack already running on :5173. Start it first with
#   ./scripts/dev-all.sh
# in a separate shell.

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
VIDEO="$ROOT/tests/e2e/fixtures/fake-cam.y4m"
AUDIO="$ROOT/tests/e2e/fixtures/fake-mic.wav"
URL="${BRIVVA_DEV_URL:-http://localhost:5173}"

if [[ ! -f "$VIDEO" || ! -f "$AUDIO" ]]; then
  echo "ERROR: fixtures missing." >&2
  echo "  expected: $VIDEO" >&2
  echo "  expected: $AUDIO" >&2
  echo "See tests/e2e/fixtures/README.md to regenerate." >&2
  exit 1
fi

# Fresh profile so real-browser cookies / extensions / signed-in
# account state don't leak into the test run.
PROFILE_DIR="$(mktemp -d)"
trap 'rm -rf "$PROFILE_DIR"' EXIT

# Pick the first available Chromium-family binary. Each accepts the
# same fake-media flags.
for CANDIDATE in \
  "/Applications/Brave Browser.app/Contents/MacOS/Brave Browser" \
  "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" \
  "/Applications/Google Chrome Canary.app/Contents/MacOS/Google Chrome Canary" \
  "/Applications/Chromium.app/Contents/MacOS/Chromium" \
  "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge" \
  "$(command -v brave-browser 2>/dev/null || true)" \
  "$(command -v google-chrome 2>/dev/null || true)" \
  "$(command -v chromium 2>/dev/null || true)"; do
  if [[ -n "$CANDIDATE" && -x "$CANDIDATE" ]]; then
    CHROME="$CANDIDATE"
    break
  fi
done
if [[ -z "${CHROME:-}" ]]; then
  echo "ERROR: no Chromium-family browser found." >&2
  exit 1
fi

echo "Launching $CHROME with fake media."
echo "  video: $VIDEO"
echo "  audio: $AUDIO"
echo "  url  : $URL"
echo

exec "$CHROME" \
  --user-data-dir="$PROFILE_DIR" \
  --no-first-run \
  --no-default-browser-check \
  --use-fake-ui-for-media-stream \
  --use-fake-device-for-media-stream \
  --use-file-for-fake-video-capture="$VIDEO" \
  --use-file-for-fake-audio-capture="$AUDIO" \
  --autoplay-policy=no-user-gesture-required \
  "$URL"
