#!/usr/bin/env bash
# Run OpenTofu with sensitive TF_VAR_* values mapped from Infisical-injected
# process environment. This writes no env files and no secret tfvars files.
#
# Usage from repo root:
#   infisical run --env=prod -- ./infra/tofu-infisical.sh plan
#   infisical run --env=prod -- ./infra/tofu-infisical.sh apply

set -euo pipefail
cd "$(dirname "$0")"

# OpenTofu's AWS provider/backend do not always understand AWS CLI v2 `login`
# credentials directly. Export them into this process when no explicit AWS env
# credentials/profile are already set. Nothing is printed or written to disk.
if [[ -z "${AWS_ACCESS_KEY_ID:-}" && -z "${AWS_PROFILE:-}" ]] && command -v aws >/dev/null 2>&1; then
    eval "$(aws configure export-credentials --format env 2>/dev/null || true)"
fi

require() {
    local key="$1"
    if [[ -z "${!key:-}" ]]; then
        echo "ERROR: $key is missing from Infisical runtime env" >&2
        exit 1
    fi
}

require SONIOX_API_KEY
require ELEVENLABS_API_KEY
require JWT_SECRET
require INTERNAL_SECRET

export TF_VAR_soniox_api_key="$SONIOX_API_KEY"
export TF_VAR_elevenlabs_api_key="$ELEVENLABS_API_KEY"
export TF_VAR_google_client_id="${GOOGLE_CLIENT_ID:-}"
export TF_VAR_google_client_secret="${GOOGLE_CLIENT_SECRET:-}"
export TF_VAR_tunnel_creds="${TUNNEL_CREDS:-}"
export TF_VAR_tunnel_id="${TUNNEL_ID:-}"
export TF_VAR_jwt_secret="$JWT_SECRET"
export TF_VAR_internal_secret="$INTERNAL_SECRET"

exec tofu "$@"
