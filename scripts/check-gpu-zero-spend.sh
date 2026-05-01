#!/usr/bin/env bash
set -euo pipefail

AWS_REGION="${AWS_REGION:-us-east-1}"
CLUSTER="${CLUSTER:-brivva}"
SERVICE="${SERVICE:-brivva-gpu}"
ASG="${ASG:-brivva-ecs-gpu}"

need() {
	if ! command -v "$1" >/dev/null 2>&1; then
		echo "Missing required command: $1" >&2
		exit 1
	fi
}
need aws

asg_json="$(aws autoscaling describe-auto-scaling-groups \
	--region "$AWS_REGION" \
	--auto-scaling-group-names "$ASG" \
	--query 'AutoScalingGroups[0].{desired:DesiredCapacity,instances:length(Instances),states:Instances[].LifecycleState}' \
	--output json)"
svc_json="$(aws ecs describe-services \
	--region "$AWS_REGION" \
	--cluster "$CLUSTER" \
	--services "$SERVICE" \
	--query 'services[0].{desired:desiredCount,running:runningCount,pending:pendingCount}' \
	--output json)"

echo "$asg_json"
echo "$svc_json"

asg_desired="$(aws autoscaling describe-auto-scaling-groups --region "$AWS_REGION" --auto-scaling-group-names "$ASG" --query 'AutoScalingGroups[0].DesiredCapacity' --output text)"
asg_instances="$(aws autoscaling describe-auto-scaling-groups --region "$AWS_REGION" --auto-scaling-group-names "$ASG" --query 'length(AutoScalingGroups[0].Instances)' --output text)"
svc_counts="$(aws ecs describe-services --region "$AWS_REGION" --cluster "$CLUSTER" --services "$SERVICE" --query 'services[0].[desiredCount,runningCount,pendingCount]' --output text)"

if [[ "$asg_desired" == "0" && "$asg_instances" == "0" && "$svc_counts" == $'0\t0\t0' ]]; then
	echo "✓ ECS GPU zero-spend confirmed"
	exit 0
fi

echo "✗ ECS GPU is not fully zero-spend/idle" >&2
echo "To scale down:" >&2
echo "  infisical run --env=prod -- ./infra/tofu-infisical.sh apply -auto-approve -var='gpu_rehearsal_service_enabled=true' -var='gpu_rehearsal_desired_count=0' -var='gpu_desired_capacity=0'" >&2
exit 2
