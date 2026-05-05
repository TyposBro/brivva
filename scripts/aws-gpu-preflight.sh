#!/usr/bin/env bash
set -euo pipefail

AWS_REGION="${AWS_REGION:-us-east-1}"
PROJECT="${PROJECT:-brivva}"
GPU_INSTANCE_TYPE="${GPU_INSTANCE_TYPE:-g4dn.xlarge}"
GPU_VCPU_QUOTA_CODE="${GPU_VCPU_QUOTA_CODE:-L-DB2E81BA}"

echo "AWS_REGION=$AWS_REGION"
echo "PROJECT=$PROJECT"
echo "GPU_INSTANCE_TYPE=$GPU_INSTANCE_TYPE"

require() {
  command -v "$1" >/dev/null 2>&1 || { echo "missing command: $1" >&2; exit 2; }
}

require aws

aws sts get-caller-identity --region "$AWS_REGION" --output json

echo "\n== EC2 GPU quota: Running On-Demand G and VT instances =="
aws service-quotas get-service-quota \
  --region "$AWS_REGION" \
  --service-code ec2 \
  --quota-code "$GPU_VCPU_QUOTA_CODE" \
  --query '{QuotaName:Quota.QuotaName,Value:Quota.Value,Unit:Quota.Unit,Adjustable:Quota.Adjustable}' \
  --output table || true

echo "\n== Default VPC/subnets/AZs =="
DEFAULT_VPC_ID="$(aws ec2 describe-vpcs \
  --region "$AWS_REGION" \
  --filters Name=is-default,Values=true \
  --query 'Vpcs[0].VpcId' \
  --output text)"
echo "DEFAULT_VPC_ID=$DEFAULT_VPC_ID"

aws ec2 describe-subnets \
  --region "$AWS_REGION" \
  --filters Name=vpc-id,Values="$DEFAULT_VPC_ID" \
  --query 'Subnets[].{SubnetId:SubnetId,AZ:AvailabilityZone,MapPublicIpOnLaunch:MapPublicIpOnLaunch,RouteTableIds:[]}' \
  --output table

echo "\n== Route tables for default VPC: check for NAT/public routes manually =="
aws ec2 describe-route-tables \
  --region "$AWS_REGION" \
  --filters Name=vpc-id,Values="$DEFAULT_VPC_ID" \
  --query 'RouteTables[].{RouteTableId:RouteTableId,Associations:Associations[].SubnetId,Routes:Routes[].{Dest:DestinationCidrBlock,Nat:NatGatewayId,Gateway:GatewayId,State:State}}' \
  --output json

echo "\n== GPU VPC endpoints present? =="
aws ec2 describe-vpc-endpoints \
  --region "$AWS_REGION" \
  --filters Name=vpc-id,Values="$DEFAULT_VPC_ID" Name=tag:Scope,Values=phase-6c-gpu-rehearsal \
  --query 'VpcEndpoints[].{VpcEndpointId:VpcEndpointId,ServiceName:ServiceName,VpcEndpointType:VpcEndpointType,State:State,SubnetIds:SubnetIds,RouteTableIds:RouteTableIds}' \
  --output table || true

echo "\n== ECS cluster/services/task defs =="
aws ecs describe-clusters --region "$AWS_REGION" --clusters "$PROJECT" --output table || true
aws ecs describe-services --region "$AWS_REGION" --cluster "$PROJECT" --services "$PROJECT" "$PROJECT-gpu" --output json || true

echo "\n== GPU ASG desired/running capacity =="
aws autoscaling describe-auto-scaling-groups \
  --region "$AWS_REGION" \
  --auto-scaling-group-names "$PROJECT-ecs-gpu" \
  --query 'AutoScalingGroups[].{Name:AutoScalingGroupName,Min:MinSize,Max:MaxSize,Desired:DesiredCapacity,Instances:Instances[].{Id:InstanceId,State:LifecycleState,Health:HealthStatus}}' \
  --output table || true

echo "\n== CloudWatch log group =="
aws logs describe-log-groups \
  --region "$AWS_REGION" \
  --log-group-name-prefix "/ecs/$PROJECT" \
  --query 'logGroups[].{Name:logGroupName,Retention:retentionInDays,StoredBytes:storedBytes}' \
  --output table || true

echo "\nPreflight is read-only. Blockers to resolve before real-platform smoke: NAT/external egress for providers/platforms and private operator/browser access, unless using primary public route."
