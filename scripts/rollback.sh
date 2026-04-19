#!/usr/bin/env bash
# Roll the Fargate service back to a prior task-definition revision.
#
# Usage:
#   ./scripts/rollback.sh            # rolls to the revision before current
#   ./scripts/rollback.sh 12         # rolls to brivva:12
#   ./scripts/rollback.sh --list     # lists recent revisions with pinned image SHA
#
# Requires: aws CLI configured for account 132593557399, region us-east-1.

set -euo pipefail

REGION="us-east-1"
CLUSTER="brivva"
SERVICE="brivva"
FAMILY="brivva"

cmd=${1:-}

if [[ "$cmd" == "--list" ]]; then
  aws ecs list-task-definitions --region "$REGION" --family-prefix "$FAMILY" --sort DESC --max-items 10 \
    --query 'taskDefinitionArns' --output text \
    | tr '\t' '\n' \
    | while read -r arn; do
        [ -z "$arn" ] && continue
        img=$(aws ecs describe-task-definition --region "$REGION" --task-definition "$arn" \
              --query "taskDefinition.containerDefinitions[?name=='server-rs']|[0].image" --output text)
        echo "$arn  →  $img"
      done
  exit 0
fi

current=$(aws ecs describe-services --region "$REGION" --cluster "$CLUSTER" --services "$SERVICE" \
          --query 'services[0].taskDefinition' --output text)
echo "current: $current"

if [[ -n "$cmd" ]]; then
  target="${FAMILY}:${cmd}"
else
  # Grab the revision immediately before the current one.
  target=$(aws ecs list-task-definitions --region "$REGION" --family-prefix "$FAMILY" --sort DESC \
           --query 'taskDefinitionArns[1]' --output text)
  if [[ "$target" == "None" || -z "$target" ]]; then
    echo "no prior revision to roll back to" >&2
    exit 1
  fi
fi

echo "rolling to: $target"
read -r -p "confirm? [y/N] " ans
[[ "$ans" =~ ^[Yy]$ ]] || { echo "aborted"; exit 1; }

aws ecs update-service --region "$REGION" --cluster "$CLUSTER" --service "$SERVICE" \
  --task-definition "$target" \
  --query 'service.taskDefinition' --output text

echo "waiting for stabilization..."
aws ecs wait services-stable --region "$REGION" --cluster "$CLUSTER" --services "$SERVICE"
echo "rollback complete → $target"
