#!/usr/bin/env bash
# Run the full-stack Playwright e2e against the real local dev stack.
#
# Preconditions (not enforced here — fail-fast messages below help):
#   1. `./scripts/dev-all.sh` running in another terminal.
#   2. `workers/.dev.vars` has DEV_AUTH_BYPASS=true.
#   3. `tests/e2e/.env` populated (copy from .env.example).
#   4. `tests/e2e/fixtures/fake-cam.y4m` + `fake-mic.wav` exist.

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
ENV_FILE="$ROOT/tests/e2e/.env"
FIXTURE_VIDEO="$ROOT/tests/e2e/fixtures/fake-cam.y4m"
FIXTURE_AUDIO="$ROOT/tests/e2e/fixtures/fake-mic.wav"

if [[ ! -f "$ENV_FILE" ]]; then
  echo "ERROR: $ENV_FILE missing." >&2
  echo "  cp tests/e2e/.env.example tests/e2e/.env" >&2
  echo "  # then fill in GRIP_* and YOUTUBE_* credentials" >&2
  exit 1
fi

if [[ ! -f "$FIXTURE_VIDEO" || ! -f "$FIXTURE_AUDIO" ]]; then
  echo "ERROR: fake-cam.y4m / fake-mic.wav missing." >&2
  echo "  see tests/e2e/fixtures/README.md to regenerate" >&2
  exit 1
fi

# Verify the dev stack is up. curl with low timeout so a typo'd
# BRIVVA_DEV_* URL surfaces immediately.
FRONTEND_URL="${BRIVVA_DEV_FRONTEND_URL:-http://localhost:5173}"
WORKERS_URL="${BRIVVA_DEV_WORKERS_URL:-http://localhost:8787}"
if ! curl -sf --max-time 2 "$FRONTEND_URL" >/dev/null; then
  echo "ERROR: frontend unreachable at $FRONTEND_URL" >&2
  echo "  start it with: ./scripts/dev-all.sh" >&2
  exit 1
fi
if ! curl -sf --max-time 2 "$WORKERS_URL/" >/dev/null; then
  echo "ERROR: workers unreachable at $WORKERS_URL" >&2
  echo "  start it with: ./scripts/dev-all.sh" >&2
  exit 1
fi

# Source the env file. `set -a` exports every assignment so the
# playwright child process inherits them.
set -a
# shellcheck disable=SC1090
. "$ENV_FILE"
set +a

cd "$ROOT/frontend"
exec bunx playwright test -c playwright.real-stack.config.ts "$@"
