#!/usr/bin/env bash
set -euo pipefail
AWS_REGION="${AWS_REGION:-us-east-1}"
PROJECT="${PROJECT:-brivva}"
aws autoscaling describe-auto-scaling-groups \
  --region "$AWS_REGION" \
  --auto-scaling-group-names "$PROJECT-ecs-gpu" \
  --query 'AutoScalingGroups[].{Name:AutoScalingGroupName,Min:MinSize,Max:MaxSize,Desired:DesiredCapacity,Instances:length(Instances)}' \
  --output table
aws ecs describe-services \
  --region "$AWS_REGION" \
  --cluster "$PROJECT" \
  --services "$PROJECT-gpu" "$PROJECT" \
  --query 'services[].{Service:serviceName,Desired:desiredCount,Running:runningCount,Pending:pendingCount,LaunchType:launchType}' \
  --output table
