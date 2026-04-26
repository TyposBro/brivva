#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

REGION="${AWS_REGION:-us-east-1}"
SECRET_ID="${AWS_SECRET_ID:-brivva/env}"
INFISICAL_PROJECT_ID="${INFISICAL_PROJECT_ID:-15f49900-322f-4724-b3b6-87f1a2436adb}"

tmp="$(mktemp)"
trap 'rm -f "$tmp"' EXIT

infisical export --env=prod --projectId="$INFISICAL_PROJECT_ID" --format=json \
  | jq 'map(select(.key as $k | [
      "SONIOX_API_KEY",
      "ELEVENLABS_API_KEY",
      "GOOGLE_CLIENT_ID",
      "GOOGLE_CLIENT_SECRET",
      "TUNNEL_CREDS",
      "JWT_SECRET",
      "INTERNAL_SECRET"
    ] | index($k))) | map(select(.value != null and .value != "")) | from_entries' \
  > "$tmp"

missing="$(jq -r '
  ["SONIOX_API_KEY","ELEVENLABS_API_KEY","GOOGLE_CLIENT_ID","GOOGLE_CLIENT_SECRET","TUNNEL_CREDS","JWT_SECRET","INTERNAL_SECRET"]
  - keys
  | .[]' "$tmp")"
if [ -n "$missing" ]; then
  printf 'missing Infisical prod keys for AWS:\n%s\n' "$missing" >&2
  exit 2
fi

aws secretsmanager update-secret \
  --region "$REGION" \
  --secret-id "$SECRET_ID" \
  --secret-string "file://$tmp" \
  >/dev/null

printf 'synced Infisical prod -> AWS Secrets Manager %s (%s)\n' "$SECRET_ID" "$REGION"
printf 'restart ECS tasks to pick up changed values if any\n'
