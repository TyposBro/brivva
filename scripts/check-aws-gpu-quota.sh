#!/usr/bin/env bash
set -euo pipefail

AWS_REGION="${AWS_REGION:-us-east-1}"
REQUIRED_VCPUS="${REQUIRED_VCPUS:-4}"
SERVICE_CODE="ec2"
QUOTA_CODE="L-DB2E81BA" # Running On-Demand G and VT instances

usage() {
	cat <<EOF
Usage: $0 [--request]

Checks AWS EC2 GPU quota for ECS g4dn.xlarge rehearsal.
Env: AWS_REGION=us-east-1 REQUIRED_VCPUS=4

--request  Request quota increase to REQUIRED_VCPUS if current quota is lower.
EOF
	exit 1
}

REQUEST=false
while [[ $# -gt 0 ]]; do
	case "$1" in
	--request)
		REQUEST=true
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
need awk

quota_json="$(aws service-quotas get-service-quota \
	--region "$AWS_REGION" \
	--service-code "$SERVICE_CODE" \
	--quota-code "$QUOTA_CODE")"

name="$(awk -F'"' '/"QuotaName"/ {print $4; exit}' <<<"$quota_json")"
value="$(awk '/"Value"/ {gsub(/,/, "", $2); print $2; exit}' <<<"$quota_json")"

echo "quota_name=$name"
echo "quota_code=$QUOTA_CODE"
echo "region=$AWS_REGION"
echo "current_vcpus=$value"
echo "required_vcpus=$REQUIRED_VCPUS"

if awk "BEGIN {exit !($value >= $REQUIRED_VCPUS)}"; then
	echo "status=ok"
	exit 0
fi

echo "status=blocked"

if [[ "$REQUEST" == true ]]; then
	echo "requesting_quota=$REQUIRED_VCPUS"
	aws service-quotas request-service-quota-increase \
		--region "$AWS_REGION" \
		--service-code "$SERVICE_CODE" \
		--quota-code "$QUOTA_CODE" \
		--desired-value "$REQUIRED_VCPUS" \
		--query 'RequestedQuota.{id:Id,status:Status,desired:DesiredValue,quota:QuotaName}' \
		--output json
fi

exit 2
