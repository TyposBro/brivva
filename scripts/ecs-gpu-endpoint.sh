#!/usr/bin/env bash
set -euo pipefail

AWS_REGION="${AWS_REGION:-us-east-1}"
CLUSTER="${CLUSTER:-brivva}"
SERVICE="${SERVICE:-brivva-gpu}"

usage() {
	cat <<EOF
Usage: $0 [--service NAME]

Print public endpoint info for the parallel ECS GPU rehearsal service.
Defaults: CLUSTER=brivva SERVICE=brivva-gpu AWS_REGION=us-east-1
EOF
	exit 1
}

while [[ $# -gt 0 ]]; do
	case "$1" in
	--service)
		SERVICE="${2:-}"
		shift 2
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
need jq

TASK_ARNS="$(aws ecs list-tasks \
	--region "$AWS_REGION" \
	--cluster "$CLUSTER" \
	--service-name "$SERVICE" \
	--desired-status RUNNING \
	--query 'taskArns[]' \
	--output text)"

if [[ -z "$TASK_ARNS" || "$TASK_ARNS" == "None" ]]; then
	echo "No RUNNING tasks for $CLUSTER/$SERVICE" >&2
	echo "Scale with: infisical run --env=prod -- ./infra/tofu-infisical.sh apply -var='gpu_rehearsal_service_enabled=true' -var='gpu_rehearsal_desired_count=1' -var='gpu_desired_capacity=1'" >&2
	exit 2
fi

TASK_JSON="$(aws ecs describe-tasks \
	--region "$AWS_REGION" \
	--cluster "$CLUSTER" \
	--tasks $TASK_ARNS)"

CONTAINER_INSTANCE_ARNS="$(jq -r '.tasks[].containerInstanceArn // empty' <<<"$TASK_JSON" | sort -u | tr '\n' ' ')"

if [[ -z "$CONTAINER_INSTANCE_ARNS" ]]; then
	echo "No EC2 container instances found for running tasks" >&2
	exit 3
fi

CI_JSON="$(aws ecs describe-container-instances \
	--region "$AWS_REGION" \
	--cluster "$CLUSTER" \
	--container-instances $CONTAINER_INSTANCE_ARNS)"

EC2_IDS="$(jq -r '.containerInstances[].ec2InstanceId' <<<"$CI_JSON" | sort -u | tr '\n' ' ')"
EC2_JSON="$(aws ec2 describe-instances \
	--region "$AWS_REGION" \
	--instance-ids $EC2_IDS)"

jq -r '
  .Reservations[].Instances[] |
  [
    "instance=" + .InstanceId,
    "state=" + .State.Name,
    "public_ip=" + (.PublicIpAddress // ""),
    "http=http://" + (.PublicIpAddress // "") + ":3000",
    "ws=ws://" + (.PublicIpAddress // "") + ":3000"
  ] | @tsv
' <<<"$EC2_JSON"
