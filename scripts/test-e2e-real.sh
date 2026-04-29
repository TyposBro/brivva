#!/usr/bin/env bash
# Run the parameterized full-stack Playwright e2e against the real
# local dev stack.
#
# Usage:
#   ./scripts/test-e2e-real.sh [flags] [-- playwright_args...]
#
# Flags:
#   --no-clone              Skip voice cloning, use default-male library voice.
#                           Faster (no ElevenLabs round-trip), no clone credits burned.
#   --source <lang>         Source spoken language. One of en|ko|ja|zh. Default: ko.
#   --grip <lang>           Add a Grip destination at <lang>. If lang == source,
#                           the destination is auto-set to passthrough (raw source audio).
#   --youtube <lang>        Add a Custom-RTMP destination at <lang> using
#                           YOUTUBE_RTMP_URL/KEY (typically a YouTube Live ingest URL).
#   --rtmp <lang>           Add a second Custom-RTMP destination using RTMP_URL/KEY.
#   --rtmp2 <lang>          Add a third Custom-RTMP destination using RTMP2_URL/KEY.
#   --instagram <lang>      Add a Custom-RTMP destination using IG_RTMP_URL/KEY.
#   --tiktok <lang>         Add a Custom-RTMP destination using TIKTOK_RTMP_URL/KEY.
#   --duration <sec>        How long to record audio after going live. Default: 90.
#   --headed                Open the browser visibly so you can watch.
#   -h, --help              This help text.
#
# Env (in tests/e2e/.env, gitignored):
#   GRIP_RTMP_URL  / GRIP_RTMP_KEY        — required if --grip set
#   YOUTUBE_RTMP_URL / YOUTUBE_RTMP_KEY   — required if --youtube set
#   RTMP_URL / RTMP_KEY                   — required if --rtmp set
#   RTMP2_URL / RTMP2_KEY                 — required if --rtmp2 set
#   IG_RTMP_URL / IG_RTMP_KEY             — required if --instagram set
#   TIKTOK_RTMP_URL / TIKTOK_RTMP_KEY     — required if --tiktok set
#
# Examples:
#   # Default-voice smoke test, two destinations:
#   ./scripts/test-e2e-real.sh --no-clone --source ko --grip zh --youtube en
#
#   # Maximum pressure (1 source → 3 translations + 1 passthrough):
#   ./scripts/test-e2e-real.sh --source ko --grip en --youtube zh --rtmp ja --rtmp2 ko
#
# See tests/e2e/README.md for combination tables.

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
ENV_FILE="$ROOT/tests/e2e/.env"
FIXTURE_VIDEO="$ROOT/tests/e2e/fixtures/fake-cam.y4m"
FIXTURE_AUDIO="$ROOT/tests/e2e/fixtures/fake-mic.wav"

# ── Defaults ──────────────────────────────────────────────
E2E_CLONE="true"
E2E_SOURCE_LANG="ko"
E2E_GRIP_LANG=""
E2E_YOUTUBE_LANG=""
E2E_RTMP_LANG=""
E2E_RTMP2_LANG=""
E2E_INSTAGRAM_LANG=""
E2E_TIKTOK_LANG=""
E2E_RECORD_SECONDS="90"
PW_EXTRA=()
HEADED=""

usage() {
  sed -n '/^# Run the parameterized/,/^$/p' "$0" | sed 's/^# \{0,1\}//'
  exit 0
}

# ── Parse flags ───────────────────────────────────────────
while [[ $# -gt 0 ]]; do
  case "$1" in
    --no-clone)        E2E_CLONE="false"; shift ;;
    --source)          E2E_SOURCE_LANG="$2"; shift 2 ;;
    --grip)            E2E_GRIP_LANG="$2"; shift 2 ;;
    --youtube)         E2E_YOUTUBE_LANG="$2"; shift 2 ;;
    --rtmp)            E2E_RTMP_LANG="$2"; shift 2 ;;
    --rtmp2)           E2E_RTMP2_LANG="$2"; shift 2 ;;
    --instagram)       E2E_INSTAGRAM_LANG="$2"; shift 2 ;;
    --tiktok)          E2E_TIKTOK_LANG="$2"; shift 2 ;;
    --duration)        E2E_RECORD_SECONDS="$2"; shift 2 ;;
    --headed)          HEADED="--headed"; shift ;;
    -h|--help)         usage ;;
    --)                shift; PW_EXTRA+=("$@"); break ;;
    *)                 PW_EXTRA+=("$1"); shift ;;
  esac
done

# At least one destination is required for the test to be meaningful.
if [[ -z "$E2E_GRIP_LANG" && -z "$E2E_YOUTUBE_LANG" && -z "$E2E_RTMP_LANG" && -z "$E2E_RTMP2_LANG" && -z "$E2E_INSTAGRAM_LANG" && -z "$E2E_TIKTOK_LANG" ]]; then
  echo "ERROR: no destinations configured." >&2
  echo "  Pass at least one of --grip/--youtube/--rtmp/--rtmp2/--instagram/--tiktok <lang>." >&2
  echo "  See $0 --help" >&2
  exit 1
fi

case "$E2E_SOURCE_LANG" in
  en|ko|ja|zh) ;;
  *) echo "ERROR: --source must be en|ko|ja|zh, got '$E2E_SOURCE_LANG'." >&2; exit 1 ;;
esac

validate_lang() {
  local flag="$1"
  local lang="$2"
  [[ -z "$lang" ]] && return 0
  case "$lang" in
    en|ko|ja|zh) ;;
    *) echo "ERROR: $flag must be en|ko|ja|zh, got '$lang'." >&2; exit 1 ;;
  esac
}
validate_lang --grip "$E2E_GRIP_LANG"
validate_lang --youtube "$E2E_YOUTUBE_LANG"
validate_lang --rtmp "$E2E_RTMP_LANG"
validate_lang --rtmp2 "$E2E_RTMP2_LANG"
validate_lang --instagram "$E2E_INSTAGRAM_LANG"
validate_lang --tiktok "$E2E_TIKTOK_LANG"

# ── Preconditions ─────────────────────────────────────────
# tests/e2e/.env is optional when credentials are supplied by the caller
# (scripts/test-e2e-auto.sh does this for its local RTMP default). Keep
# supporting the file for real Grip/YouTube credentials.

if [[ ! -f "$FIXTURE_VIDEO" || ! -f "$FIXTURE_AUDIO" ]]; then
  echo "WARN: fake-cam.y4m / fake-mic.wav missing." >&2
  echo "  Chromium will still use --use-fake-device-for-media-stream," >&2
  echo "  but file-backed fake media needs tests/e2e/fixtures/README.md" >&2
  echo "  or scripts/test-e2e-auto.sh to generate fixtures." >&2
fi

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

# ── Source .env, layer e2e config on top ──────────────────
set -a
if [[ -f "$ENV_FILE" ]]; then
  # shellcheck disable=SC1090
  . "$ENV_FILE"
fi
# Fail fast for any destination flag whose creds are missing — easier
# than discovering the placeholder mid-test.
require_creds() {
  local flag="$1"
  local lang="$2"
  local url_var="$3"
  local key_var="$4"
  [[ -z "$lang" ]] && return 0
  local url="${!url_var:-}"
  local key="${!key_var:-}"
  if [[ -z "$url" || "$url" == YOUR-* || -z "$key" || "$key" == YOUR_* ]]; then
    echo "ERROR: $flag set to '$lang' but $url_var / $key_var missing or placeholder in $ENV_FILE." >&2
    exit 1
  fi
}
require_creds --grip    "$E2E_GRIP_LANG"    GRIP_RTMP_URL    GRIP_RTMP_KEY
require_creds --youtube "$E2E_YOUTUBE_LANG" YOUTUBE_RTMP_URL YOUTUBE_RTMP_KEY
require_creds --rtmp      "$E2E_RTMP_LANG"      RTMP_URL         RTMP_KEY
require_creds --rtmp2     "$E2E_RTMP2_LANG"     RTMP2_URL        RTMP2_KEY
require_creds --instagram "$E2E_INSTAGRAM_LANG" IG_RTMP_URL      IG_RTMP_KEY
require_creds --tiktok    "$E2E_TIKTOK_LANG"    TIKTOK_RTMP_URL  TIKTOK_RTMP_KEY

export E2E_CLONE
export E2E_SOURCE_LANG
export E2E_GRIP_LANG
export E2E_YOUTUBE_LANG
export E2E_RTMP_LANG
export E2E_RTMP2_LANG
export E2E_INSTAGRAM_LANG
export E2E_TIKTOK_LANG
export E2E_RECORD_SECONDS
set +a

# ── Echo plan so the operator sees the matrix at a glance ─
echo "──────────────── e2e configuration ────────────────"
printf "  clone        : %s\n" "$E2E_CLONE"
printf "  source lang  : %s\n" "$E2E_SOURCE_LANG"
[[ -n "$E2E_GRIP_LANG"    ]] && printf "  grip         : %s%s\n" "$E2E_GRIP_LANG"    "$([ "$E2E_GRIP_LANG"    = "$E2E_SOURCE_LANG" ] && echo ' (auto-passthrough)' || true)"
[[ -n "$E2E_YOUTUBE_LANG" ]] && printf "  youtube/rtmp1: %s%s\n" "$E2E_YOUTUBE_LANG" "$([ "$E2E_YOUTUBE_LANG" = "$E2E_SOURCE_LANG" ] && echo ' (auto-passthrough)' || true)"
[[ -n "$E2E_RTMP_LANG"    ]] && printf "  rtmp2        : %s%s\n" "$E2E_RTMP_LANG"    "$([ "$E2E_RTMP_LANG"    = "$E2E_SOURCE_LANG" ] && echo ' (auto-passthrough)' || true)"
[[ -n "$E2E_RTMP2_LANG"   ]] && printf "  rtmp3        : %s%s\n" "$E2E_RTMP2_LANG"   "$([ "$E2E_RTMP2_LANG"   = "$E2E_SOURCE_LANG" ] && echo ' (auto-passthrough)' || true)"
[[ -n "$E2E_INSTAGRAM_LANG" ]] && printf "  instagram    : %s%s\n" "$E2E_INSTAGRAM_LANG" "$([ "$E2E_INSTAGRAM_LANG" = "$E2E_SOURCE_LANG" ] && echo ' (auto-passthrough)' || true)"
[[ -n "$E2E_TIKTOK_LANG"    ]] && printf "  tiktok       : %s%s\n" "$E2E_TIKTOK_LANG"    "$([ "$E2E_TIKTOK_LANG"    = "$E2E_SOURCE_LANG" ] && echo ' (auto-passthrough)' || true)"
printf "  record secs  : %s\n" "$E2E_RECORD_SECONDS"
echo "───────────────────────────────────────────────────"

cd "$ROOT/frontend"
# `${arr[@]+"${arr[@]}"}` is the macOS-bash-3.2-safe way to expand a
# possibly-empty array under `set -u` — plain `"${arr[@]}"` errors with
# "unbound variable" when no extra Playwright args were passed.
exec bunx playwright test -c playwright.real-stack.config.ts $HEADED ${PW_EXTRA[@]+"${PW_EXTRA[@]}"}
