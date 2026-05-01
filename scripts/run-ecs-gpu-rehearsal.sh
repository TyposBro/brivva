#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
AWS_REGION="${AWS_REGION:-us-east-1}"
GPU_INSTANCE_TYPE="${GPU_INSTANCE_TYPE:-g4dn.xlarge}"
FRONTEND_PORT="${FRONTEND_PORT:-5173}"
WORKERS_API_URL="${WORKERS_API_URL:-https://brivva-api.milliytechnology.workers.dev}"
LOG_DIR="$REPO_ROOT/.dev-logs"
mkdir -p "$LOG_DIR"

PREFLIGHT_ONLY=false
NO_FRONTEND=false
NO_SCALE_DOWN=false

usage() {
	cat <<EOF
Usage: $0 [--preflight-only] [--no-frontend] [--no-scale-down]

Scale the parallel ECS GPU rehearsal service to 1, discover the direct media
endpoint, smoke /health, optionally start local frontend pointed at it, then
scale back to zero on exit.

Env:
  AWS_REGION=us-east-1
  GPU_INSTANCE_TYPE=g4dn.xlarge
  FRONTEND_PORT=5173
  WORKERS_API_URL=https://brivva-api.milliytechnology.workers.dev

Flags:
  --preflight-only  Check quota/Terraform/scripts only; no AWS scale-up.
  --no-frontend    Scale/smoke ECS GPU only; do not start local frontend.
  --no-scale-down  Leave ECS GPU running after exit (costs money).
EOF
	exit 1
}

while [[ $# -gt 0 ]]; do
	case "$1" in
	--preflight-only)
		PREFLIGHT_ONLY=true
		shift
		;;
	--no-frontend)
		NO_FRONTEND=true
		shift
		;;
	--no-scale-down)
		NO_SCALE_DOWN=true
		shift
		;;
	-h | --help) usage ;;
	*)
		echo "Unknown arg: $1" >&2
		usage
		;;
	esac
done

need() {
	if ! command -v "$1" >/dev/null 2>&1; then
		echo "Missing required command: $1" >&2
		exit 1
	fi
}
need aws
need curl
need infisical
need terraform
if [[ "$NO_FRONTEND" != true ]]; then need bun; fi

wait_for_zero() {
	for i in {1..60}; do
		instances="$(aws autoscaling describe-auto-scaling-groups --region "$AWS_REGION" --auto-scaling-group-names brivva-ecs-gpu --query 'length(AutoScalingGroups[0].Instances)' --output text)"
		tasks="$(aws ecs describe-services --region "$AWS_REGION" --cluster brivva --services brivva-gpu --query 'services[0].[runningCount,pendingCount]' --output text)"
		echo "==> wait zero instances=$instances tasks=[$tasks] poll=$i"
		if [[ "$instances" == "0" && "$tasks" == $'0\t0' ]]; then return 0; fi
		sleep 10
	done
	return 1
}

cleanup() {
	if [[ -n "${FRONTEND_PID:-}" ]]; then kill "$FRONTEND_PID" 2>/dev/null || true; fi
	if [[ "$NO_SCALE_DOWN" != true && "$PREFLIGHT_ONLY" != true ]]; then
		echo "==> Scaling ECS GPU rehearsal back to zero"
		infisical run --env=prod -- "$REPO_ROOT/infra/tofu-infisical.sh" apply -auto-approve \
			-var='gpu_rehearsal_service_enabled=true' \
			-var='gpu_rehearsal_desired_count=0' \
			-var='gpu_desired_capacity=0'
		wait_for_zero || echo "WARN: GPU scale-down still draining; check AWS console/ASG"
	fi
}
trap cleanup EXIT INT TERM

"$REPO_ROOT/scripts/check-aws-gpu-quota.sh"
terraform -chdir="$REPO_ROOT/infra" validate
bash -n \
	"$REPO_ROOT/scripts/ecs-gpu-endpoint.sh" \
	"$REPO_ROOT/scripts/smoke-media-engine.sh" \
	"$REPO_ROOT/scripts/collect-rehearsal-proof.sh"

if [[ "$PREFLIGHT_ONLY" == true ]]; then
	echo "✓ ECS GPU rehearsal preflight complete"
	exit 0
fi

if [[ "$NO_FRONTEND" != true ]] && lsof -iTCP:"$FRONTEND_PORT" -sTCP:LISTEN >/dev/null 2>&1; then
	echo "Frontend port $FRONTEND_PORT already in use" >&2
	lsof -iTCP:"$FRONTEND_PORT" -sTCP:LISTEN >&2
	exit 2
fi

infisical run --env=prod -- "$REPO_ROOT/infra/tofu-infisical.sh" apply -auto-approve \
	-var='gpu_rehearsal_service_enabled=true' \
	-var='gpu_rehearsal_desired_count=1' \
	-var='gpu_desired_capacity=1' \
	-var="gpu_instance_type=$GPU_INSTANCE_TYPE"

for i in {1..60}; do
	running="$(aws ecs describe-services --region "$AWS_REGION" --cluster brivva --services brivva-gpu --query 'services[0].runningCount' --output text)"
	pending="$(aws ecs describe-services --region "$AWS_REGION" --cluster brivva --services brivva-gpu --query 'services[0].pendingCount' --output text)"
	echo "==> wait brivva-gpu running=$running pending=$pending poll=$i"
	if [[ "$running" == "1" ]]; then break; fi
	sleep 10
done

endpoint_line="$($REPO_ROOT/scripts/ecs-gpu-endpoint.sh)"
echo "$endpoint_line"
MEDIA_URL="$(awk -F'\t' 'NR==1{for(i=1;i<=NF;i++) if($i ~ /^http=/){sub(/^http=/,"",$i); print $i}}' <<<"$endpoint_line")"
if [[ -z "$MEDIA_URL" ]]; then
	echo "Could not discover ECS GPU media URL" >&2
	exit 3
fi

"$REPO_ROOT/scripts/smoke-media-engine.sh" "$MEDIA_URL"
echo "$MEDIA_URL" >"$LOG_DIR/ecs-gpu-media-url.txt"

if [[ "$NO_FRONTEND" == true ]]; then
	echo "✓ ECS GPU media endpoint ready: $MEDIA_URL"
	exit 0
fi

(
	cd "$REPO_ROOT/frontend"
	VITE_API_URL="$WORKERS_API_URL" \
		VITE_MEDIA_URL="$MEDIA_URL" \
		VITE_WORKER_URL="$MEDIA_URL" \
		bun run dev --host 0.0.0.0 --port "$FRONTEND_PORT"
) >"$LOG_DIR/ecs-gpu-rehearsal-frontend.log" 2>&1 &
FRONTEND_PID=$!

FRONTEND_URL="http://localhost:$FRONTEND_PORT"
for i in {1..60}; do
	if curl -fsS "$FRONTEND_URL" >/dev/null 2>&1; then break; fi
	if ! kill -0 "$FRONTEND_PID" 2>/dev/null; then
		echo "Frontend exited early; tail log:" >&2
		tail -80 "$LOG_DIR/ecs-gpu-rehearsal-frontend.log" >&2
		exit 4
	fi
	sleep 1
done

cat <<EOF
✓ ECS GPU rehearsal stack ready
frontend_url=$FRONTEND_URL
media_url=$MEDIA_URL
workers_api_url=$WORKERS_API_URL
frontend_log=$LOG_DIR/ecs-gpu-rehearsal-frontend.log

Next manual step:
  Open $FRONTEND_URL, start host session, stream 30s+, then run:
  ./scripts/collect-rehearsal-proof.sh <SESSION_ID> --remote

Press Ctrl-C to stop frontend and scale ECS GPU back to zero.
EOF

wait
