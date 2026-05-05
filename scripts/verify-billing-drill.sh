#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage:
  scripts/verify-billing-drill.sh checklist
  BASE_URL=http://127.0.0.1:8787 INTERNAL_SECRET=dev USER_ID=billing-drill scripts/verify-billing-drill.sh api [--allow-remote]

Local-only billing drill for RS-006/RS-007. No AWS, no real platform, no prod secrets.
Default API mode refuses non-localhost unless --allow-remote is explicit.
USAGE
}

mode="${1:-checklist}"
allow_remote="${2:-}"

if [[ "$mode" == "-h" || "$mode" == "--help" ]]; then
  usage
  exit 0
fi

if [[ "$mode" == "checklist" ]]; then
  cat <<'CHECKLIST'
# RS-006/RS-007 Billing Drill Checklist

Use on local Worker/D1 or captured AWS rehearsal artifacts only. Do not mutate prod secrets.

Verify four billing facts:
1. Bad Soniox source/lang window: POST provider failure with provider=soniox, scope=lang, lang=<affected>, billable=false.
2. Bad ElevenLabs TTS/lang window: provider=elevenlabs, scope=lang, lang=<affected>, billable=false.
3. Bad RTMP/platform output window: provider=rtmp, scope=output, output_id=<affected>, platform=<platform>, lang=<affected>, billable=false.
4. Healthy siblings remain billable: usage/summary still reports positive billable minutes for unaffected langs/outputs.

Local automated command:
  BASE_URL=http://127.0.0.1:8787 INTERNAL_SECRET=<dev-secret> USER_ID=billing-drill scripts/verify-billing-drill.sh api

Unit test command:
  bun run --cwd workers test -- tests/api.test.ts -t "billing drill keeps healthy siblings billable"
CHECKLIST
  exit 0
fi

if [[ "$mode" != "api" ]]; then
  usage >&2
  exit 2
fi

base_url="${BASE_URL:-http://127.0.0.1:8787}"
internal_secret="${INTERNAL_SECRET:-}"
user_id="${USER_ID:-billing-drill-local}"

if [[ -z "$internal_secret" ]]; then
  echo "INTERNAL_SECRET required" >&2
  exit 2
fi

if [[ "$base_url" != http://127.0.0.1:* && "$base_url" != http://localhost:* && "$allow_remote" != "--allow-remote" ]]; then
  echo "Refusing non-local BASE_URL without --allow-remote: $base_url" >&2
  exit 2
fi

json_post() {
  local path="$1"
  local body="$2"
  curl -fsS "$base_url$path" \
    -H 'Content-Type: application/json' \
    -H "X-Internal-Secret: $internal_secret" \
    -d "$body"
}

api_post() {
  local path="$1"
  local body="$2"
  curl -fsS "$base_url$path" -H 'Content-Type: application/json' -d "$body"
}

created="$(api_post /api/sessions "{\"user_id\":\"$user_id\",\"title\":\"Billing drill\",\"source_lang\":\"en\",\"target_langs\":[\"ja\",\"ko\",\"zh\"]}")"
session_id="$(python3 -c 'import json,sys; print(json.load(sys.stdin)["session"]["id"])' <<<"$created")"

echo "session_id=$session_id"

json_post "/internal/sessions/$session_id/metrics" '{"source_seconds":180,"output_seconds_by_lang":{"ja":180,"ko":180,"zh":180}}' >/dev/null

post_failure() {
  json_post "/internal/sessions/$session_id/provider-failures" "$1" >/dev/null
}

post_failure '{"live_session_id":"live-billing-drill","provider":"soniox","scope":"lang","state":"failed","reason":"bad_source_lang_or_key","recoverable":true,"billable":false,"lang":"ja","started_at_ms":1000,"recovered_at_ms":61000,"message":"bad Soniox window: JA translation unbillable"}'
post_failure '{"live_session_id":"live-billing-drill","provider":"elevenlabs","scope":"lang","state":"failed","reason":"bad_tts_lang_or_key","recoverable":true,"billable":false,"lang":"ko","status_code":401,"started_at_ms":1000,"recovered_at_ms":61000,"message":"bad ElevenLabs window: KO TTS unbillable"}'
post_failure '{"live_session_id":"live-billing-drill","output_id":"rtmp-zh-youtube","provider":"rtmp","scope":"output","state":"failed","reason":"bad_platform_key","recoverable":true,"billable":false,"lang":"zh","platform":"youtube","started_at_ms":1000,"recovered_at_ms":61000,"message":"bad RTMP/platform window: ZH output unbillable"}'

usage_json="$(curl -fsS "$base_url/api/sessions/$session_id/usage")"
USAGE_JSON="$usage_json" python3 - <<'PY'
import json, os, sys
body = json.loads(os.environ["USAGE_JSON"])
expected_outputs = {"ja": 2, "ko": 2, "zh": 2}
expected_providers = {"soniox": 1, "elevenlabs": 1, "rtmp": 1}
expected_langs = {"ja": 1, "ko": 1, "zh": 1}
errors = []
for k, v in expected_outputs.items():
    got = body.get("output_minutes_by_lang", {}).get(k)
    if got != v:
        errors.append(f"output_minutes_by_lang.{k}: got {got}, want {v}")
for k, v in expected_providers.items():
    got = body.get("unbillable_windows", {}).get("by_provider_minutes", {}).get(k)
    if got != v:
        errors.append(f"unbillable provider {k}: got {got}, want {v}")
for k, v in expected_langs.items():
    got = body.get("unbillable_windows", {}).get("by_lang_minutes", {}).get(k)
    if got != v:
        errors.append(f"unbillable lang {k}: got {got}, want {v}")
if body.get("estimated_cost_usd") != 9:
    errors.append(f"estimated_cost_usd: got {body.get('estimated_cost_usd')}, want 9")
if errors:
    print("ASSERT FAIL")
    print("\n".join(errors))
    sys.exit(1)
print("ASSERT OK billing drill: bad Soniox, bad ElevenLabs, bad RTMP unbillable; siblings remain billable")
PY
