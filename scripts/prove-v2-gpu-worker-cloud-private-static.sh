#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MAIN_TF="$ROOT/infra/main.tf"
ENDPOINT_SH="$ROOT/scripts/ecs-gpu-endpoint.sh"

fail() {
	echo "✗ $*" >&2
	exit 1
}

need_file() {
	[[ -f "$1" ]] || fail "missing file: $1"
}

need_file "$MAIN_TF"
need_file "$ENDPOINT_SH"

# 6C private-only guard: GPU launch template must not assign public IPs.
awk '
  /resource "aws_launch_template" "ecs_gpu"/ { in_lt=1 }
  in_lt && /resource "aws_autoscaling_group" "ecs_gpu"/ { in_lt=0 }
  in_lt && /associate_public_ip_address[[:space:]]*=[[:space:]]*true/ { bad=1 }
  END { exit bad ? 1 : 0 }
' "$MAIN_TF" || fail "ecs_gpu launch template enables public IP assignment"

awk '
  /resource "aws_launch_template" "ecs_gpu"/ { in_lt=1 }
  in_lt && /resource "aws_autoscaling_group" "ecs_gpu"/ { in_lt=0 }
  in_lt && /associate_public_ip_address[[:space:]]*=[[:space:]]*false/ { ok=1 }
  END { exit ok ? 0 : 1 }
' "$MAIN_TF" || fail "ecs_gpu launch template does not explicitly disable public IP assignment"

# 6C private-only guard: TCP 3000 must not be open to the world.
awk '
  /from_port[[:space:]]*=[[:space:]]*3000/ { in_3000=1; window=0 }
  in_3000 && /cidr_blocks[[:space:]]*=[[:space:]]*\["0\.0\.0\.0\/0"\]/ { bad=1 }
  in_3000 { window++ }
  window > 8 { in_3000=0 }
  END { exit bad ? 1 : 0 }
' "$MAIN_TF" || fail "TCP 3000 ingress is open to 0.0.0.0/0"

grep -q 'cidr_blocks = \[data.aws_vpc.default.cidr_block\]' "$MAIN_TF" || fail "TCP 3000 ingress is not limited to VPC CIDR"

# Endpoint helper must never print public http/ws endpoint fields.
if grep -Eq 'http=http://.*PublicIpAddress|ws=ws://.*PublicIpAddress|public_ip=.*http' "$ENDPOINT_SH"; then
	fail "endpoint helper still emits public http/ws endpoint"
fi
grep -q 'Refusing to emit endpoint: GPU rehearsal instance has a public IP' "$ENDPOINT_SH" || fail "endpoint helper does not refuse public IPs"
grep -q 'http_private=http://' "$ENDPOINT_SH" || fail "endpoint helper does not emit private http endpoint"
grep -q 'ws_private=ws://' "$ENDPOINT_SH" || fail "endpoint helper does not emit private ws endpoint"

# 6C must be shadow/fake-sink only and production GPU routing default off.
grep -q 'BRIVVA_V2_GPU_WORKERS", value = "0"' "$MAIN_TF" || fail "BRIVVA_V2_GPU_WORKERS is not default-off for GPU rehearsal"
grep -q 'BRIVVA_V2_GPU_REHEARSAL_MODE", value = "shadow"' "$MAIN_TF" || fail "GPU rehearsal mode is not shadow"
grep -q 'BRIVVA_V2_GPU_REHEARSAL_SINK", value = "fake"' "$MAIN_TF" || fail "GPU rehearsal sink is not fake"

# Static publish guard: the rehearsal container block must not include RTMP/S publish destinations.
awk '
  /gpu_rehearsal_server_container = merge/ { in_gpu=1 }
  in_gpu && /gpu_rehearsal_containers =/ { in_gpu=0 }
  in_gpu && /rtmp:\/\/|rtmps:\/\// { bad=1 }
  END { exit bad ? 1 : 0 }
' "$MAIN_TF" || fail "GPU rehearsal container contains non-fake RTMP publish destination"

# Private endpoint support must be gated and scoped to GPU rehearsal only.
grep -q 'variable "gpu_private_endpoints_enabled"' "$ROOT/infra/variables.tf" || fail "missing gpu_private_endpoints_enabled gate"
grep -q 'default     = false' "$ROOT/infra/variables.tf" || fail "gpu private endpoints are not default-off"
grep -q 'gpu_subnet_ids = var.gpu_private_endpoints_enabled ? slice(data.aws_subnets.gpu.ids, 0, 1)' "$MAIN_TF" || fail "GPU private endpoint mode is not constrained to one GPU subnet/AZ"
grep -q 'resource "aws_security_group" "gpu_private_endpoints"' "$MAIN_TF" || fail "missing GPU private endpoint security group"
grep -q 'security_groups = \[aws_security_group.task.id\]' "$MAIN_TF" || fail "endpoint SG is not limited to GPU/task security group source"
grep -q 'resource "aws_vpc_endpoint" "gpu_private_interface"' "$MAIN_TF" || fail "missing GPU private interface endpoints"
grep -q 'resource "aws_vpc_endpoint" "gpu_private_s3"' "$MAIN_TF" || fail "missing GPU private S3 gateway endpoint"
grep -q 'gpu_private_endpoint_route_table_ids must explicitly list the GPU subnet route table' "$MAIN_TF" || fail "S3 gateway endpoint does not require explicit GPU route table IDs"

for svc in ecs ecs-agent ecs-telemetry ecr.api ecr.dkr logs secretsmanager; do
	grep -q "\"$svc\"" "$MAIN_TF" || fail "missing required private endpoint service: $svc"
done

if grep -q 'route_table_ids[[:space:]]*=.*data.aws_route' "$MAIN_TF"; then
	fail "S3 gateway endpoint route tables are inferred instead of explicit"
fi

bash -n "$ENDPOINT_SH"

echo "✓ Phase 6C private-only static proof passed"
