#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage: scripts/plan-ecs-gpu-bad-provider-drill.sh soniox|elevenlabs [extra tofu args...]

Plans an isolated brivva-gpu bad-provider drill. Refuses to touch shared
production brivva/env by forcing Terraform to create/use brivva/env-gpu-drill
for the rehearsal task definition only.

This is plan-only: no deploy, no scale-up, no provider traffic.
USAGE
}

provider="${1:-}"
if [[ -z "$provider" || "$provider" == "-h" || "$provider" == "--help" ]]; then
  usage
  exit 0
fi
shift || true

case "$provider" in
  soniox|elevenlabs) ;;
  *)
    echo "Refusing: provider must be soniox or elevenlabs" >&2
    exit 2
    ;;
esac

if [[ " ${*:-} " == *" brivva/env "* || " ${*:-} " == *"env-gpu-drill=false"* ]]; then
  echo "Refusing: drill must not target shared brivva/env or disable isolated drill secret." >&2
  exit 2
fi

cat <<EOF
Guardrail: planning isolated brivva-gpu ${provider} drill only.
- primary service: brivva (unchanged)
- primary secret: brivva/env (must not be mutated for bad keys)
- drill service: brivva-gpu
- drill secret: brivva/env-gpu-drill
- desired counts forced to 0 in this plan; no AWS scale-up/deploy/stream.
EOF

infisical run --env=prod -- ./infra/tofu-infisical.sh plan \
  -var='gpu_rehearsal_service_enabled=true' \
  -var='gpu_rehearsal_desired_count=0' \
  -var='gpu_desired_capacity=0' \
  -var="gpu_bad_provider_drill=${provider}" \
  -var='gpu_bad_provider_sentinel=ISOLATED_GPU_DRILL_ONLY' \
  "$@"
