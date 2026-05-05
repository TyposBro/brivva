#!/usr/bin/env bash
set -euo pipefail
AWS_REGION="${AWS_REGION:-us-east-1}"
QUOTA_CODE="${GPU_VCPU_QUOTA_CODE:-L-DB2E81BA}"
MIN_VCPU="${MIN_GPU_VCPU:-4}"

usage() { echo "usage: AWS_REGION=us-east-1 $0 [--request]" >&2; }
REQUEST=0
if [[ "${1:-}" == "--request" ]]; then REQUEST=1; elif [[ $# -gt 0 ]]; then usage; exit 2; fi

if [[ "$REQUEST" == 1 ]]; then
  aws service-quotas request-service-quota-increase \
    --region "$AWS_REGION" \
    --service-code ec2 \
    --quota-code "$QUOTA_CODE" \
    --desired-value "$MIN_VCPU"
  exit 0
fi

aws service-quotas get-service-quota \
  --region "$AWS_REGION" \
  --service-code ec2 \
  --quota-code "$QUOTA_CODE" \
  --query '{QuotaName:QuotaName,Value:Value,Unit:Unit,NeededVcpu:`'"$MIN_VCPU"'`}' \
  --output table
