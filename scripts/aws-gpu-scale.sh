#!/usr/bin/env bash
# Scale Brivva ECS GPU media stack up/down in AWS us-east-1.
# No image deploy here; deploy.sh owns task-definition revisions.

set -euo pipefail

AWS_REGION="${AWS_REGION:-us-east-1}"
CLUSTER="${CLUSTER:-brivva}"
SERVICE="${SERVICE:-brivva}"
REHEARSAL_SERVICE="${REHEARSAL_SERVICE:-brivva-gpu}"
ASG="${ASG:-brivva-ecs-gpu}"

usage() {
  echo "Usage: $0 start|stop|status" >&2
  exit 1
}

cmd="${1:-}"
[[ -n "$cmd" ]] || usage

case "$cmd" in
  start)
    echo "==> Scaling ASG $ASG to 1 GPU instance"
    aws autoscaling update-auto-scaling-group \
      --region "$AWS_REGION" \
      --auto-scaling-group-name "$ASG" \
      --min-size 0 --max-size 1 --desired-capacity 1

    echo "==> Waiting for ECS container instance registration"
    for _ in {1..60}; do
      count="$(aws ecs list-container-instances \
        --region "$AWS_REGION" \
        --cluster "$CLUSTER" \
        --status ACTIVE \
        --query 'length(containerInstanceArns)' \
        --output text 2>/dev/null || echo 0)"
      [[ "$count" != "0" ]] && break
      sleep 10
    done

    echo "==> Scaling ECS service $SERVICE to 1 task"
    aws ecs update-service \
      --region "$AWS_REGION" \
      --cluster "$CLUSTER" \
      --service "$SERVICE" \
      --desired-count 1 >/dev/null
    aws ecs wait services-stable \
      --region "$AWS_REGION" \
      --cluster "$CLUSTER" \
      --services "$SERVICE"
    ;;

  stop)
    echo "==> Scaling ECS services to 0 tasks"
    aws ecs update-service \
      --region "$AWS_REGION" \
      --cluster "$CLUSTER" \
      --service "$SERVICE" \
      --desired-count 0 >/dev/null || true
    aws ecs update-service \
      --region "$AWS_REGION" \
      --cluster "$CLUSTER" \
      --service "$REHEARSAL_SERVICE" \
      --desired-count 0 >/dev/null || true
    aws ecs wait services-stable \
      --region "$AWS_REGION" \
      --cluster "$CLUSTER" \
      --services "$SERVICE" "$REHEARSAL_SERVICE" || true

    echo "==> Scaling ASG $ASG to 0 GPU instances"
    aws autoscaling update-auto-scaling-group \
      --region "$AWS_REGION" \
      --auto-scaling-group-name "$ASG" \
      --min-size 0 --max-size 1 --desired-capacity 0
    ;;

  status)
    echo "==> ASG"
    aws autoscaling describe-auto-scaling-groups \
      --region "$AWS_REGION" \
      --auto-scaling-group-names "$ASG" \
      --query 'AutoScalingGroups[].{Min:MinSize,Max:MaxSize,Desired:DesiredCapacity,Instances:length(Instances)}' \
      --output table
    echo "==> ECS services"
    aws ecs describe-services \
      --region "$AWS_REGION" \
      --cluster "$CLUSTER" \
      --services "$SERVICE" "$REHEARSAL_SERVICE" \
      --query 'services[].{Name:serviceName,Desired:desiredCount,Running:runningCount,Pending:pendingCount,Status:status}' \
      --output table
    ;;

  *) usage ;;
esac
