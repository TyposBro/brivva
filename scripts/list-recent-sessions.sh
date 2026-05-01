#!/usr/bin/env bash
set -euo pipefail

ORIGINAL_ARGS=("$@")
SCOPE="${1:---remote}"
LIMIT="${LIMIT:-10}"

usage() {
	cat <<EOF
Usage: $0 [--remote|--local]

List recent Brivva sessions from D1 so you can find SESSION_ID for:
  ./scripts/verify-session-burn-in.sh <SESSION_ID> --remote
  ./scripts/collect-rehearsal-proof.sh <SESSION_ID> --remote
EOF
	exit 1
}

case "$SCOPE" in
--remote | --local) ;;
-h | --help) usage ;;
*)
	echo "scope must be --remote or --local" >&2
	usage
	;;
esac

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WORKERS_DIR="$REPO_ROOT/workers"

if [[ "${BRIVVA_INFISICAL_WRAPPED:-0}" != "1" && "$SCOPE" == "--remote" && -z "${CLOUDFLARE_API_TOKEN:-}" ]] && command -v infisical >/dev/null 2>&1; then
	export BRIVVA_INFISICAL_WRAPPED=1
	exec infisical run --env="${INFISICAL_ENV:-prod}" --path="${INFISICAL_PATH:-/}" --project-config-dir "$REPO_ROOT" -- "$0" "${ORIGINAL_ARGS[@]}"
fi

command -v bun >/dev/null 2>&1 || {
	echo "bun not found" >&2
	exit 1
}

SQL="SELECT id,status,COALESCE(live_session_id, room_id, '') AS live_session_id,title,datetime(CASE WHEN created_at > 100000000000 THEN created_at/1000 ELSE created_at END,'unixepoch') AS created_utc FROM sessions ORDER BY created_at DESC LIMIT $LIMIT;"
(cd "$WORKERS_DIR" && bun wrangler d1 execute brivva "$SCOPE" --command "$SQL")
