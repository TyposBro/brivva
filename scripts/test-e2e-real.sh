#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ENV_FILE="$ROOT/tests/e2e/.env"

usage() {
	cat <<'EOF'
Usage: ./scripts/test-e2e-real.sh [flags]

Flags:
  --no-clone
  --source en|ko|ja|zh
  --grip en|ko|ja|zh
  --youtube en|ko|ja|zh
  --rtmp en|ko|ja|zh
  --rtmp2 en|ko|ja|zh
  --instagram en|ko|ja|zh
  --tiktok en|ko|ja|zh
  --duration seconds
  --headed

Credentials can come from tests/e2e/.env or the current environment.
EOF
}

is_lang() {
	case "${1:-}" in
		en | ko | ja | zh) return 0 ;;
		*) return 1 ;;
	esac
}

set_lang_flag() {
	local env_name="$1"
	local value="${2:-}"
	if ! is_lang "$value"; then
		echo "$env_name must be one of en|ko|ja|zh, got '$value'" >&2
		exit 2
	fi
	export "$env_name=$value"
}

require_pair() {
	local prefix="$1"
	local lang_env="$2"
	if [[ -z "${!lang_env:-}" ]]; then
		return 0
	fi
	local url_name="${prefix}_URL"
	local key_name="${prefix}_KEY"
	if [[ -z "${!url_name:-}" || -z "${!key_name:-}" ]]; then
		echo "$url_name/$key_name required for $lang_env=${!lang_env}" >&2
		exit 2
	fi
}

if [[ -f "$ENV_FILE" ]]; then
	set -a
	# shellcheck disable=SC1090
	source "$ENV_FILE"
	set +a
fi

export E2E_CLONE="${E2E_CLONE:-true}"
export E2E_SOURCE_LANG="${E2E_SOURCE_LANG:-ko}"
export E2E_RECORD_SECONDS="${E2E_RECORD_SECONDS:-90}"
HEADED=0

while [[ $# -gt 0 ]]; do
	case "$1" in
		--help | -h)
			usage
			exit 0
			;;
		--no-clone)
			export E2E_CLONE=false
			shift
			;;
		--source)
			set_lang_flag E2E_SOURCE_LANG "${2:-}"
			shift 2
			;;
		--grip)
			set_lang_flag E2E_GRIP_LANG "${2:-}"
			shift 2
			;;
		--youtube)
			set_lang_flag E2E_YOUTUBE_LANG "${2:-}"
			shift 2
			;;
		--rtmp)
			set_lang_flag E2E_RTMP_LANG "${2:-}"
			shift 2
			;;
		--rtmp2)
			set_lang_flag E2E_RTMP2_LANG "${2:-}"
			shift 2
			;;
		--instagram)
			set_lang_flag E2E_INSTAGRAM_LANG "${2:-}"
			shift 2
			;;
		--tiktok)
			set_lang_flag E2E_TIKTOK_LANG "${2:-}"
			shift 2
			;;
		--duration)
			export E2E_RECORD_SECONDS="${2:-}"
			shift 2
			;;
		--headed)
			HEADED=1
			shift
			;;
		*)
			echo "unknown flag: $1" >&2
			usage >&2
			exit 2
			;;
	esac
done

set_lang_flag E2E_SOURCE_LANG "$E2E_SOURCE_LANG"
if ! [[ "$E2E_RECORD_SECONDS" =~ ^[0-9]+$ ]]; then
	echo "E2E_RECORD_SECONDS must be integer seconds" >&2
	exit 2
fi

require_pair GRIP_RTMP E2E_GRIP_LANG
require_pair YOUTUBE_RTMP E2E_YOUTUBE_LANG
require_pair RTMP E2E_RTMP_LANG
require_pair RTMP2 E2E_RTMP2_LANG
require_pair IG_RTMP E2E_INSTAGRAM_LANG
require_pair TIKTOK_RTMP E2E_TIKTOK_LANG

args=(test --config "$ROOT/frontend/playwright.real-stack.config.ts")
if [[ "$HEADED" == "1" ]]; then
	args+=(--headed)
fi

cd "$ROOT/frontend"
bunx playwright "${args[@]}"
