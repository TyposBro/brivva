#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"
INFISICAL_PROJECT_ID="${INFISICAL_PROJECT_ID:-15f49900-322f-4724-b3b6-87f1a2436adb}"

keys=(
  ELEVENLABS_API_KEY
  GOOGLE_CLIENT_ID
  GOOGLE_CLIENT_SECRET
  JWT_SECRET
  INTERNAL_SECRET
)

optional_keys=(
  GRIP_ACCESS_KEY
  GRIP_SECRET_KEY
  STRIPE_WEBHOOK_SECRET
)

tmp="$(mktemp)"
trap 'rm -f "$tmp"' EXIT

infisical export --env=prod --projectId="$INFISICAL_PROJECT_ID" --format=json \
  | jq -r '.[] | select(.value != null and .value != "") | "\(.key)=\(.value)"' \
  > "$tmp"

missing=()
for key in "${keys[@]}"; do
  if ! awk -F= -v k="$key" '$1 == k { found=1 } END { exit found ? 0 : 1 }' "$tmp"; then
    missing+=("$key")
  fi
done

if [ "${#missing[@]}" -ne 0 ]; then
  printf 'missing Infisical prod keys for Workers:\n' >&2
  printf '  %s\n' "${missing[@]}" >&2
  exit 2
fi

for key in "${keys[@]}"; do
  awk -F= -v k="$key" '$1 == k { print substr($0, index($0, "=") + 1); exit }' "$tmp" \
    | bunx wrangler secret put "$key" --config workers/wrangler.toml
done

for key in "${optional_keys[@]}"; do
  if awk -F= -v k="$key" '$1 == k { found=1 } END { exit found ? 0 : 1 }' "$tmp"; then
    awk -F= -v k="$key" '$1 == k { print substr($0, index($0, "=") + 1); exit }' "$tmp" \
      | bunx wrangler secret put "$key" --config workers/wrangler.toml
  fi
done

printf 'synced Infisical prod -> Cloudflare Workers secrets\n'
