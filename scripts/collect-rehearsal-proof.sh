#!/usr/bin/env bash
set -euo pipefail

ORIGINAL_ARGS=("$@")
SESSION_ID="${1:-}"
SCOPE="${2:---remote}"
ENGINE_LOG="${BRIVVA_ENGINE_LOG:-.dev-logs/laptop-rehearsal-engine.log}"
FRONTEND_LOG="${BRIVVA_FRONTEND_LOG:-.dev-logs/laptop-rehearsal-frontend.log}"
AWS_REGION="${AWS_REGION:-us-east-1}"

usage() {
	cat <<EOF
Usage: $0 <SESSION_ID> [--remote|--local]

Collect proof for laptop/ECS GPU rehearsal:
- D1 session/metrics/log verification
- NVENC/FFmpeg/NVIDIA evidence
- media engine logs
- AWS GPU quota state
- git revision

Output: .dev-logs/rehearsals/<SESSION_ID>/
EOF
	exit 1
}

if [[ -z "$SESSION_ID" || "$SESSION_ID" == "-h" || "$SESSION_ID" == "--help" ]]; then
	usage
fi
case "$SCOPE" in
--remote | --local) ;;
*)
	echo "scope must be --remote or --local" >&2
	usage
	;;
esac

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

if [[ "${BRIVVA_INFISICAL_WRAPPED:-0}" != "1" && "$SCOPE" == "--remote" && -z "${CLOUDFLARE_API_TOKEN:-}" ]] && command -v infisical >/dev/null 2>&1; then
	export BRIVVA_INFISICAL_WRAPPED=1
	exec infisical run --env="${INFISICAL_ENV:-prod}" --path="${INFISICAL_PATH:-/}" --project-config-dir "$REPO_ROOT" -- "$0" "${ORIGINAL_ARGS[@]}"
fi

OUT_DIR="$REPO_ROOT/.dev-logs/rehearsals/$SESSION_ID"
mkdir -p "$OUT_DIR"

run_capture() {
	local name="$1"
	shift
	{
		echo "# $name"
		echo "# $(date -Is)"
		echo "+ $*"
		"$@"
	} >"$OUT_DIR/$name.txt" 2>&1 || {
		local code=$?
		echo "command failed ($code): $*" >>"$OUT_DIR/$name.txt"
		return "$code"
	}
}

cp_if_exists() {
	local src="$1"
	local dest="$2"
	if [[ -f "$REPO_ROOT/$src" ]]; then
		cp "$REPO_ROOT/$src" "$OUT_DIR/$dest"
	elif [[ -f "$src" ]]; then
		cp "$src" "$OUT_DIR/$dest"
	fi
}

{
	echo "session_id=$SESSION_ID"
	echo "scope=$SCOPE"
	echo "created_at=$(date -Is)"
	echo "git_sha=$(git -C "$REPO_ROOT" rev-parse HEAD 2>/dev/null || true)"
	echo "engine_log=$ENGINE_LOG"
	echo "frontend_log=$FRONTEND_LOG"
} >"$OUT_DIR/manifest.txt"

BRIVVA_ENGINE_LOG="$ENGINE_LOG" "$REPO_ROOT/scripts/verify-session-burn-in.sh" "$SESSION_ID" "$SCOPE" \
	>"$OUT_DIR/verify-session-burn-in.txt" 2>&1 || {
	code=$?
	echo "verify-session-burn-in failed ($code); see $OUT_DIR/verify-session-burn-in.txt" >&2
	exit "$code"
}

cp_if_exists "$ENGINE_LOG" "engine.log"
cp_if_exists "$FRONTEND_LOG" "frontend.log"

run_capture "ffmpeg-version" ffmpeg -hide_banner -version || true
run_capture "ffmpeg-nvenc" bash -lc "ffmpeg -hide_banner -encoders 2>/dev/null | grep h264_nvenc; ffmpeg -hide_banner -protocols 2>&1 | grep rtmps; ffmpeg -hide_banner -filters 2>&1 | grep drawtext" || true
if command -v nvidia-smi >/dev/null 2>&1; then
	run_capture "nvidia-smi" nvidia-smi || true
fi
if command -v aws >/dev/null 2>&1; then
	run_capture "aws-gpu-quota" aws service-quotas list-requested-service-quota-change-history-by-quota --region "$AWS_REGION" --service-code ec2 --quota-code L-DB2E81BA --query 'RequestedQuotas[0].{id:Id,status:Status,desired:DesiredValue,created:Created}' --output json || true
fi

if command -v tar >/dev/null 2>&1; then
	tar -czf "$OUT_DIR.tar.gz" -C "$OUT_DIR" .
	echo "proof_dir=$OUT_DIR"
	echo "proof_archive=$OUT_DIR.tar.gz"
else
	echo "proof_dir=$OUT_DIR"
fi
