#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage:
  scripts/aws-soak-run-artifacts.sh init --profile NAME [--root tmp/aws-soak-runs] [--run-id YYYYMMDD-HHMM-NAME]
  scripts/aws-soak-run-artifacts.sh export-logs --run-dir DIR --log-group GROUP --start ISO8601 --end ISO8601 [--region us-east-1] [--service-prefix PREFIX] [--output FILE]

Read-only helper for RS-007 AWS soak artifacts. It creates local templates and exports
CloudWatch logs with aws logs filter-log-events. It does not deploy, scale, or mutate AWS.

Examples:
  scripts/aws-soak-run-artifacts.sh init --profile fake-sink-gpu
  scripts/aws-soak-run-artifacts.sh export-logs \
    --run-dir tmp/aws-soak-runs/20260505-1530-fake-sink-gpu \
    --region us-east-1 \
    --log-group /ecs/brivva/server-rs \
    --service-prefix brivva-gpu \
    --start 2026-05-05T06:30:00Z \
    --end 2026-05-05T07:00:00Z
USAGE
}

need_arg() {
  local name="$1" value="${2:-}"
  if [[ -z "$value" ]]; then
    echo "missing value for $name" >&2
    exit 2
  fi
}

iso_to_ms() {
  local iso="$1"
  date -u -d "$iso" +%s000
}

write_if_missing() {
  local path="$1"
  if [[ -e "$path" ]]; then
    echo "keep existing $path"
    return 0
  fi
  cat > "$path"
  echo "wrote $path"
}

cmd_init() {
  local profile="" root="tmp/aws-soak-runs" run_id=""
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --profile) profile="${2:-}"; need_arg "$1" "$profile"; shift 2 ;;
      --root) root="${2:-}"; need_arg "$1" "$root"; shift 2 ;;
      --run-id) run_id="${2:-}"; need_arg "$1" "$run_id"; shift 2 ;;
      -h|--help) usage; exit 0 ;;
      *) echo "unknown init arg: $1" >&2; exit 2 ;;
    esac
  done
  need_arg --profile "$profile"
  if [[ -z "$run_id" ]]; then
    run_id="$(date -u +%Y%m%d-%H%M)-$profile"
  fi
  local run_dir="$root/$run_id"
  mkdir -p "$run_dir"

  local commit="unknown"
  if git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
    commit="$(git rev-parse HEAD)"
  fi

  write_if_missing "$run_dir/manifest.md" <<EOF
# AWS Soak Manifest

Run: $run_id
Profile: $profile
Commit: $commit
Created UTC: $(date -u +%Y-%m-%dT%H:%M:%SZ)
Created KST: $(TZ=Asia/Seoul date +%Y-%m-%dT%H:%M:%S%z)

## AWS / ECS

- Region: us-east-1
- Cluster: brivva
- Service: brivva-gpu / brivva
- Network mode: fake-sink private rehearsal / private NAT real-platform / primary EC2_GPU cutover
- Task definition ARN:
- Task ARN:
- Container instance ARN:
- EC2 instance ID / type / AZ:
- Auto Scaling group desired/running after run:

## Image

- Git SHA tag:
- Image URI:
- Image digest actually pulled:
- Proof command/output file:

## Runtime Capability

- FFmpeg version/config proof:
- Protocols include rtmp/rtmps:
- OpenSSL/native RTMPS proof:
- Filters include drawtext:
- Encoders include h264_nvenc and AAC:
- FFprobe version:
- CJK font proof:
- NVENC smoke proof:

## Fixture / Launch Shape

- Source fixture path:
- Source fixture sha256:
- Input cap: 1080p30
- Output profile: H.264 720x1280 portrait, 1s keyframes, ~2.5 Mbps
- Encoder: nvenc
- Platforms:
- Languages:
- Stream key freshness notes:

## Timing

- Start UTC:
- End UTC:
- Start KST:
- End KST:
- Duration minutes:
EOF

  write_if_missing "$run_dir/platform-visible-live.md" <<'EOF'
# Platform Visible Live

## Dashboard Checks

| Platform | Output/lang | Visible video | Audible source/translated audio | Captions/subtitles | Health green | Checked by | UTC time | Notes |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| YouTube | pass/source | yes/no | yes/no | yes/no | yes/no |  |  |  |
| YouTube | translated | yes/no | yes/no | yes/no | yes/no |  |  |  |
| Grip | launch output | yes/no | yes/no | yes/no | yes/no |  |  |  |
| TikTok/generic RTMP | launch output | yes/no | yes/no | yes/no | yes/no |  |  |  |

## Human Listening

- Source/pass:
- JA:
- ZH:
- KO/source-lang:
- Sync/latency notes:
- CJK tofu/subtitle notes:
EOF

  write_if_missing "$run_dir/billing-check.md" <<'EOF'
# Billing Check

Session ID:
Output IDs:
Run window UTC:

## Metrics / Usage Inputs

- Workers metrics/session logs received: yes/no
- Usage/summary artifact:
- Provider health windows artifact:

## Assertions

| Check | Expected | Actual | Pass |
| --- | --- | --- | --- |
| Clean healthy minutes billable | delivered healthy window |  | yes/no |
| Bad Soniox window unbillable | translated/source-lang affected only |  | yes/no/n-a |
| Bad ElevenLabs window unbillable | TTS/lang affected only |  | yes/no/n-a |
| Bad RTMP/platform window unbillable | failed output/platform only |  | yes/no/n-a |
| Healthy siblings remain billable | unaffected outputs billable |  | yes/no/n-a |

Product/SKU caveats:
EOF

  write_if_missing "$run_dir/verdict.md" <<'EOF'
# AWS Soak Verdict

Run:
Commit:
Image digest:
Task definition:
Instance:
Duration:
Platforms:
Languages:

## Result

PASS / FAIL

## Hard Signals

- FFmpeg restarts: 0 / N
- Sustained below realtime: 0 / N
- video_stale_chunks_dropped: 0 / N
- host_audio_stale_chunks_dropped: 0 / N
- ready_host_bytes_dropped: 0 / N
- TTS overflows by lang: {...}
- hard recovery by lang: {...}
- Workers metrics/session logs: yes/no
- Billing failure windows: yes/no/not-applicable

## Platform Visible Live

- YouTube pass/source: yes/no
- YouTube JA/ZH/etc: yes/no
- Grip: yes/no
- TikTok/generic RTMP: yes/no

## Human Listening

- Source: ok/bad
- JA: ok/bad
- ZH: ok/bad
- KO/source-lang: ok/bad
- Notes:

## First Bad Timestamp If Failed

UTC timestamp:
Log line:
Likely class: AWS / FFmpeg / platform / Soniox / ElevenLabs / billing / operator

## Decision

- RS-007 status: open/watch/patched
- Next patch:
EOF

  touch "$run_dir/terraform-plan.txt" \
    "$run_dir/ecs-describe-services.json" \
    "$run_dir/ecs-describe-tasks.json" \
    "$run_dir/ecs-container-instance.json" \
    "$run_dir/smoke.log" \
    "$run_dir/soak.log" \
    "$run_dir/provider-drills.log"

  echo "$run_dir"
}

cmd_export_logs() {
  local run_dir="" region="${AWS_REGION:-us-east-1}" log_group="" start="" end="" service_prefix="" output=""
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --run-dir) run_dir="${2:-}"; need_arg "$1" "$run_dir"; shift 2 ;;
      --region) region="${2:-}"; need_arg "$1" "$region"; shift 2 ;;
      --log-group) log_group="${2:-}"; need_arg "$1" "$log_group"; shift 2 ;;
      --start) start="${2:-}"; need_arg "$1" "$start"; shift 2 ;;
      --end) end="${2:-}"; need_arg "$1" "$end"; shift 2 ;;
      --service-prefix) service_prefix="${2:-}"; need_arg "$1" "$service_prefix"; shift 2 ;;
      --output) output="${2:-}"; need_arg "$1" "$output"; shift 2 ;;
      -h|--help) usage; exit 0 ;;
      *) echo "unknown export-logs arg: $1" >&2; exit 2 ;;
    esac
  done
  need_arg --run-dir "$run_dir"
  need_arg --log-group "$log_group"
  need_arg --start "$start"
  need_arg --end "$end"
  mkdir -p "$run_dir"

  local safe_group
  safe_group="$(echo "$log_group" | tr '/:' '__')"
  if [[ -z "$output" ]]; then
    if [[ -n "$service_prefix" ]]; then
      output="$run_dir/cloudwatch-${service_prefix}.log"
    else
      output="$run_dir/cloudwatch-${safe_group}.log"
    fi
  fi

  local start_ms end_ms args
  start_ms="$(iso_to_ms "$start")"
  end_ms="$(iso_to_ms "$end")"
  args=(logs filter-log-events --region "$region" --log-group-name "$log_group" --start-time "$start_ms" --end-time "$end_ms")
  if [[ -n "$service_prefix" ]]; then
    args+=(--log-stream-name-prefix "$service_prefix")
  fi
  args+=(--output json)

  {
    echo "# CloudWatch export"
    echo "# region=$region"
    echo "# log_group=$log_group"
    echo "# service_prefix=$service_prefix"
    echo "# start=$start ($start_ms)"
    echo "# end=$end ($end_ms)"
    echo "# command=aws ${args[*]}"
    aws "${args[@]}" | jq -r '.events[] | [.timestamp, .logStreamName, .message] | @tsv' 2>/dev/null || \
      aws "${args[@]}"
  } > "$output"
  echo "$output"
}

main() {
  local cmd="${1:-}"
  case "$cmd" in
    init) shift; cmd_init "$@" ;;
    export-logs) shift; cmd_export_logs "$@" ;;
    -h|--help|"") usage ;;
    *) echo "unknown command: $cmd" >&2; usage >&2; exit 2 ;;
  esac
}

main "$@"
