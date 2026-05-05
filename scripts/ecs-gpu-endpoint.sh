#!/usr/bin/env bash
set -euo pipefail
AWS_REGION="${AWS_REGION:-us-east-1}"
PROJECT="${PROJECT:-brivva}"
TASK_ARN="${TASK_ARN:-}"

if [[ -z "$TASK_ARN" ]]; then
  TASK_ARN="$(aws ecs list-tasks --region "$AWS_REGION" --cluster "$PROJECT" --service-name "$PROJECT-gpu" --desired-status RUNNING --query 'taskArns[0]' --output text)"
fi
if [[ -z "$TASK_ARN" || "$TASK_ARN" == "None" ]]; then
  echo "no running $PROJECT-gpu task found" >&2; exit 1
fi

INSTANCE_ARN="$(aws ecs describe-tasks --region "$AWS_REGION" --cluster "$PROJECT" --tasks "$TASK_ARN" --query 'tasks[0].containerInstanceArn' --output text)"
EC2_ID="$(aws ecs describe-container-instances --region "$AWS_REGION" --cluster "$PROJECT" --container-instances "$INSTANCE_ARN" --query 'containerInstances[0].ec2InstanceId' --output text)"
PRIVATE_IP="$(aws ec2 describe-instances --region "$AWS_REGION" --instance-ids "$EC2_ID" --query 'Reservations[0].Instances[0].PrivateIpAddress' --output text)"
PUBLIC_IP="$(aws ec2 describe-instances --region "$AWS_REGION" --instance-ids "$EC2_ID" --query 'Reservations[0].Instances[0].PublicIpAddress' --output text)"
if [[ "$PUBLIC_IP" != "None" && -n "$PUBLIC_IP" ]]; then
  echo "refusing public endpoint: instance has PublicIpAddress=$PUBLIC_IP" >&2; exit 1
fi
printf 'http://%s:3000\nws://%s:3000\n' "$PRIVATE_IP" "$PRIVATE_IP"
