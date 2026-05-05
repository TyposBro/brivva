# infra — Terraform (AWS us-east-1)

Reproducible AWS infra for Brivva backend. Region fixed to `us-east-1` for proximity to Soniox + ElevenLabs.

## What it provisions

| Resource               | Name                   | Notes                                                                                   |
| ---------------------- | ---------------------- | --------------------------------------------------------------------------------------- |
| ECR repo               | `brivva/server-rs`     | Keep last 10 images (lifecycle policy)                                                  |
| Secrets Manager secret | `brivva/env`           | Runtime secret payload, populated from Infisical during `tofu apply`                    |
| IAM role               | `brivva-ecs-execution` | ECS task exec + `secretsmanager:GetSecretValue` on `brivva/env`                         |
| CloudWatch log group   | `/ecs/brivva`          | 7d retention                                                                            |
| ECS cluster            | `brivva`               | Fargate CPU fallback or ECS-on-EC2 GPU primary                                          |
| ECS task def           | `brivva`               | 16 vCPU / 32 GB fallback; GPU mode requests 1 GPU and sets `BRIVVA_VIDEO_ENCODER=nvenc` |
| ECS service            | `brivva`               | 1 task; Fargate launch type or GPU capacity provider                                    |
| Security group         | `brivva-task`          | Egress + WebRTC UDP 40000-40100                                                         |

Sensitive values live in Infisical. Terraform receives them only as
`TF_VAR_*` process environment variables via `infra/tofu-infisical.sh`; no
`.env`, `.dev.vars`, or secret `terraform.tfvars` files are used.

## First apply

```fish
# 1. Install OpenTofu (or HashiCorp Terraform)
brew install opentofu

# 2. Configure AWS creds for an account with admin-ish perms
aws configure   # or: set -x AWS_PROFILE brivva

# 3. Init + apply with secrets injected by Infisical
cd infra
tofu init
cd ..
infisical run --env=prod -- ./infra/tofu-infisical.sh apply
```

Outputs include `ecr_server_url`, `account_id`, etc.

## ECS GPU mode (May 10 primary)

Default Terraform remains `ecs_launch_type=FARGATE` and `gpu_desired_capacity=0`, so no GPU instance bill appears until explicitly enabled.

Safer blue/green rehearsal: create a parallel `brivva-gpu` service first. This does **not** replace the primary Fargate `brivva` service.

Phase 6C GPU rehearsal is private/internal-only: no public IP assignment, no TCP 3000 from `0.0.0.0/0`, `BRIVVA_V2_GPU_WORKERS=0`, and fake-sink/shadow mode only. The endpoint helper refuses to print public HTTP/WS endpoints.

No-public-IP GPU ECS capacity needs private AWS control-plane access. The low-blast-radius rehearsal path is VPC endpoints, gated by `gpu_private_endpoints_enabled=false` by default. When enabled, Terraform creates one-AZ interface endpoints for `ecs`, `ecs-agent`, `ecs-telemetry`, `ecr.api`, `ecr.dkr`, `logs`, and `secretsmanager`, plus an S3 gateway endpoint for ECR image layers. Optional SSM debug endpoints are behind `gpu_private_endpoint_debug_services_enabled`. You must explicitly pass `gpu_private_endpoint_route_table_ids=[...]` for the GPU subnet route table(s); Terraform intentionally refuses to infer or mutate all default VPC route tables.

Read-only AWS preflight:

```fish
set -x AWS_REGION us-east-1
./scripts/aws-gpu-preflight.sh
./scripts/check-aws-gpu-quota.sh
```

This does not apply Terraform, scale EC2, deploy, or mutate secrets. It only describes identity, quota, default VPC/subnets/route tables, rehearsal VPC endpoints, ECS services, GPU ASG, and CloudWatch log group.

Lower-level Terraform commands:

```fish
# reviewed 6C endpoint prep only: creates private endpoints, keeps GPU desired/task count at 0
# replace rtb-... with the reviewed GPU subnet route table id(s)
infisical run --env=prod -- ./infra/tofu-infisical.sh plan \
  -var='gpu_rehearsal_service_enabled=true' \
  -var='gpu_rehearsal_desired_count=0' \
  -var='gpu_desired_capacity=0' \
  -var='gpu_private_endpoints_enabled=true' \
  -var='gpu_private_endpoint_route_table_ids=["rtb-..."]'

# create parallel GPU service/task definition with desired_count=0
infisical run --env=prod -- ./infra/tofu-infisical.sh apply \
  -var='gpu_rehearsal_service_enabled=true' \
  -var='gpu_rehearsal_desired_count=0'

# pre-check EC2 GPU quota; must be >=4 vCPU for g4dn.xlarge
./scripts/check-aws-gpu-quota.sh

# request quota increase when blocked
./scripts/check-aws-gpu-quota.sh --request

# when ready to pay and smoke test, start one GPU host + one GPU task
infisical run --env=prod -- ./infra/tofu-infisical.sh apply \
  -var='gpu_rehearsal_service_enabled=true' \
  -var='gpu_rehearsal_desired_count=1' \
  -var='gpu_desired_capacity=1' \
  -var='gpu_instance_type=g4dn.xlarge'

# after task is RUNNING, print private/internal HTTP/WS endpoint for VITE_MEDIA_URL testing
# from a network that can reach the VPC. Refuses public IP endpoints.
AWS_REGION=us-east-1 ./scripts/ecs-gpu-endpoint.sh

# no dedicated AWS media-smoke helper exists yet; use the real API/frontend from
# the operator private network or add a narrow ECS-side fake-sink smoke entrypoint.
# Do not call private fake-sink proof production-equivalent.

# if quota/capacity blocks launch or smoke is done, return to zero spend/retry loop
infisical run --env=prod -- ./infra/tofu-infisical.sh apply \
  -var='gpu_rehearsal_service_enabled=true' \
  -var='gpu_rehearsal_desired_count=0' \
  -var='gpu_desired_capacity=0'

# verify no GPU EC2 spend remains
AWS_REGION=us-east-1 ./scripts/check-gpu-zero-spend.sh
```

### Real-platform blocker matrix

Current rehearsal network path is private/no-public-IP and fake-sink/shadow by default:

- Launch template sets `associate_public_ip_address = false`.
- VPC endpoints cover AWS control-plane traffic only: ECS, ECS agent/telemetry, ECR API/DKR, CloudWatch Logs, Secrets Manager, and S3 image layers.
- VPC endpoints do **not** provide external internet egress to Soniox, ElevenLabs, YouTube, Grip, TikTok, or generic RTMP destinations.
- Operator/browser access to private `http://<gpu-private-ip>:3000` requires VPN, bastion, SSM port-forward, or another private route. SSM debugging also needs `gpu_private_endpoint_debug_services_enabled=true` plus IAM/session-manager permissions; ECS Exec is not enabled by this Terraform.
- Therefore private `brivva-gpu` without NAT/operator route can prove only GPU/image/CloudWatch/secrets/fake-sink capability. It cannot close RS-007 or prove paid production.

Real-platform smoke requires one of these explicit choices before scaling paid GPU capacity:

1. private `brivva-gpu` + NAT/external egress + private operator/browser access;
2. controlled primary `brivva` EC2_GPU cutover using the existing public route/tunnel, with rollback ready;
3. future bastion/VPN/SSM-tunnel design plus external egress.

Direct primary switch path (riskier): changes the existing `brivva` service from Fargate to EC2 GPU and may replace the service.

```fish
infisical run --env=prod -- ./infra/tofu-infisical.sh apply \
  -var='ecs_launch_type=EC2_GPU' \
  -var='gpu_desired_capacity=1' \
  -var='gpu_instance_type=g4dn.xlarge'
```

Turn GPU spend back off:

```fish
infisical run --env=prod -- ./infra/tofu-infisical.sh apply \
  -var='ecs_launch_type=EC2_GPU' \
  -var='gpu_desired_capacity=0'
```

GPU runtime requirements:

```fish
./scripts/check-ffmpeg-runtime.sh nvenc
```

The GPU path uses:

- ECS GPU-optimized AL2 AMI from SSM.
- ASG + ECS capacity provider.
- Task `resourceRequirements = GPU:1`.
- `BRIVVA_VIDEO_ENCODER=nvenc`.
- WebRTC UDP `40000-40100` directly on the task/instance security group.

## Redeploy (after code change)

Terraform does NOT rebuild images. CPU/Fargate fallback flow:

```fish
cd ..
./deploy.sh   # build → push :latest → force new ECS deployment
```

GPU/ECS-on-EC2 image flow:

```fish
# first GPU image bootstrap / after GPU Dockerfile changes
./deploy.sh --gpu --build-ffmpeg-gpu-base --build-server-bases --bases-only

# build/push GPU server image without touching ECS services
./deploy.sh --gpu --build-only

# direct primary deploy only after cutover is intended
./deploy.sh --gpu
```

`--gpu` selects the NVENC runtime base and writes `BRIVVA_VIDEO_ENCODER=nvenc` into the task definition. The server build base is shared with CPU deploys; the runtime base tag defaults to `bookworm-ffmpeg-7.1.1-nvenc-v1`.

### Session Naming Migration

If your deployed Workers D1 database predates the `room_id` → `live_session_id`
rename, run the one-off migration before deploying code that expects the new
column name:

```fish
cd workers
bun run ops:migrate-session-rename:prod
```

The SQL lives in [workers/ops/live-session-id-migration.sql](../workers/ops/live-session-id-migration.sql).

`deploy.sh` auto-resolves account from terraform state. Override with `AWS_ACCOUNT=...` if running standalone.

## Isolated bad-provider drills

Bad Soniox/ElevenLabs drills must never edit Infisical prod values or the shared
Secrets Manager secret `brivva/env`. The primary ECS service `brivva` always
references `brivva/env`; drill poisoning is isolated to the parallel `brivva-gpu`
task definition by creating `brivva/env-gpu-drill` and pointing only the
rehearsal container at that secret.

Plan-only guardrail commands (no scale-up, no deploy, desired counts forced to
0):

```fish
./scripts/plan-ecs-gpu-bad-provider-drill.sh soniox
./scripts/plan-ecs-gpu-bad-provider-drill.sh elevenlabs
```

Manual equivalent:

```fish
infisical run --env=prod -- ./infra/tofu-infisical.sh plan \
  -var='gpu_rehearsal_service_enabled=true' \
  -var='gpu_rehearsal_desired_count=0' \
  -var='gpu_desired_capacity=0' \
  -var='gpu_bad_provider_drill=soniox' \
  -var='gpu_bad_provider_sentinel=ISOLATED_GPU_DRILL_ONLY'
```

Proof of isolation to check in the Terraform plan/task definitions:

- `aws_ecs_task_definition.app` / service `brivva` secret ARNs stay on
  `brivva/env`.
- `aws_ecs_task_definition.gpu_rehearsal` / service `brivva-gpu` uses
  `brivva/env-gpu-drill` only when `gpu_bad_provider_drill != none`.
- `brivva/env` secret value is not changed to a bad provider key; only
  `brivva/env-gpu-drill` contains the sentinel bad key.
- `gpu_bad_provider_sentinel=ISOLATED_GPU_DRILL_ONLY` is required or Terraform
  validation fails.

## Rotate secrets

Update the value in Infisical, then re-apply the AWS secret payload and
restart tasks:

```fish
infisical run --env=prod -- ./infra/tofu-infisical.sh apply
./deploy.sh --skip-build
```

Tasks fetch secrets at container start — they don't hot-reload.

## Sizing

**Current default: 16 vCPU / 32 GB.** (bumped from 8/16 on 2026-04-30 after
YouTube RTMP fell below realtime on 720p30, and from 2/4 on 2026-04-20 to carry
the May 10 "ultimate test" shape: En source → Ko+Zh+Ja translations +
passthrough on Grip+TikTok+YouTube simultaneously.)

Rough budget per running stream:

| Work                            | CPU per process | RAM per process |
| ------------------------------- | --------------- | --------------- |
| Audio-only encode (AAC)         | ~5% of 1 core   | 50 MB           |
| Video passthrough (`-c:v copy`) | ~5% of 1 core   | 50 MB           |
| libx264 720p30 transcode        | ~50% of 1 core  | 200 MB          |
| libx264 1080p30 transcode       | ~80% of 1 core  | 400 MB          |
| + drawtext subtitle overlay     | +5-10%          | negligible      |

**One stream = one ffmpeg process per RTMP destination** (source + K translated languages = 1+K processes).

### Can a 16 vCPU / 32 GB task handle it?

| Scenario                                                                                 | Fit                                                                                           |
| ---------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| 1 stream, 1080p transcode, 3 translations + passthrough + burn-in subs ("ultimate test") | ✅ ~3.45 cores budgeted, with >12 cores headroom for encoder variance + STT/TTS orchestration |
| 1 stream, 1080p transcode, 5 translations + passthrough                                  | ✅ ~5.1 cores budgeted, still comfortable                                                     |
| 2 concurrent host sessions, 1080p transcode, 3 translations each                         | ✅ ~7 cores budgeted, but prefer horizontal scale-out for isolation once multi-room launches  |
| 3 concurrent host sessions                                                               | ❌ needs horizontal scale-out (see `docs/horizontal-scaling-plan.md` — Phase 2)               |

### Tune via Terraform config

If you need to override the new default:

```hcl
task_cpu    = "16384"  # 16 vCPU (current default)
task_memory = "32768"  # 32 GB
```

Valid Fargate combos: see [AWS docs](https://docs.aws.amazon.com/AmazonECS/latest/developerguide/task-cpu-memory-error.html). Common pairs: 1024/2048, 2048/4096, 4096/8192, 8192/16384, 16384/32768.

Apply changes: `tofu apply`, then `./deploy.sh --skip-build` to roll tasks.

## Session log capture

Per-session logs are disabled by default. When enabled, frontend + Workers +
server-rs write structured events into D1 and expose them as NDJSON from:

```txt
GET /api/sessions/<SESSION_ID>/logs.ndjson?user_id=<USER_ID>
```

Toggle layers independently:

| Layer                              | Flag                                                                         | Default |
| ---------------------------------- | ---------------------------------------------------------------------------- | ------- |
| Workers persistence/export         | `SESSION_LOGS_ENABLED=0/1` in `workers/wrangler.toml` or Worker vars         | on      |
| Frontend upload                    | `VITE_SESSION_LOGS=0/1` at build time or `localStorage.brivva:sessionLogs=1` | on      |
| server-rs upload                   | `session_logs_enabled` Terraform variable                                    | on      |
| server-rs high-volume debug events | `session_logs_verbose` Terraform variable                                    | off     |

Keep verbose off for normal production. Frontend media stats are enough for
session-quality estimates; server verbose is only for debugging Fargate-side
event ordering.

## Horizontal scale (many concurrent rooms)

A **single task handles a single host session well** but not many. The pipeline holds per-host state (ffmpeg child procs, SQLite row locks, WS connections, ElevenLabs voice clone id). Scale **horizontally** — one task per room, not a fatter task.

### What this requires (not done yet — future work)

1. **Stateless routing.** Move voice-clone IDs, active langs, RTMP creds from in-memory `Rooms` map to a shared store (DynamoDB, or keep SQLite and replace with RDS Postgres / Aurora Serverless).

2. **Session affinity.** Host WS must stick to one task for the whole broadcast. Options:
   - ALB with `sticky sessions` (cookie-based) + WS target group
   - Cloudflare Worker routing `room_id → task_ip` via Service Connect / internal DNS
   - Dedicated task per host (precreate on stream start API call)

3. **Service autoscaling.** Replace `desired_count=1` with target-tracking policy on `ActiveConnectionCount` or custom CloudWatch metric (`active_rooms`). Start with `min=1, max=10, target=1 room/task`.

4. **ECS Service Connect or Cloud Map.** For task-to-task discovery if you need task pinning.

### Cheap path if concurrency is low

If you expect ≤3 concurrent hosts, just bump vertically to 4 vCPU / 8 GB and keep `desired_count=1`. Revisit when you see >60% CPU sustained.

### When to scale to many tasks

Signal: **any** of these

- Average CPU >70% for >5min
- ffmpeg process restart rate climbs (dropped frames under load)
- > 2 concurrent hosts routinely

## Subtitles architecture

Live subtitle support is uneven across platforms:

| Platform                                               | Text caption ingest | How                                 |
| ------------------------------------------------------ | ------------------- | ----------------------------------- |
| YouTube Live                                           | ✅                  | HTTP POST per-broadcast caption URL |
| Facebook Live                                          | ✅                  | Graph API captions endpoint         |
| Twitch                                                 | ⚠️                  | CEA-608/708 embedded as H.264 SEI   |
| Instagram / TikTok / Kuaishou / Bilibili / custom RTMP | ❌                  | —                                   |

**Decision: burn in subtitles universally** (via ffmpeg `drawtext` filter). Writing per-platform caption code for only 2-3 platforms is more work than one burn-in path that covers everything.

### How burn-in fits

Each language's RTMP output already runs its own ffmpeg process. Add a `drawtext` filter to that same process:

```
-vf "drawtext=textfile=/tmp/subs-<lang>.txt:reload=1:fontfile=...:fontcolor=white:borderw=2:bordercolor=black:x=(w-text_w)/2:y=h-80"
```

Pipeline writes the translated text per utterance to `/tmp/subs-<lang>.txt`. `reload=1` makes ffmpeg re-read every ~1s. Clear the file at utterance end to blank the overlay.

**No extra ffmpeg processes.** Same 1+K process count.

**Source-language stream** gets the original transcript; translated streams get translated text. Both paths already have the text in the pipeline — just plumb it to the file + add the filter.

### Cost

~5-10% CPU per process on top of the existing transcode. If the stream is passthrough (source browser sends H.264), enabling drawtext forces a re-encode — adds ~50% CPU. Factor this in when budgeting.

### Gotchas

- **Fontconfig.** ffmpeg with drawtext needs a font. Current Dockerfile must `apt install fonts-dejavu-core fontconfig` (and run `fc-cache -f` once) or drawtext fails with "no suitable fonts found". Past commit `8417dfd` fixed this before — re-check Dockerfile after migration.
- **CJK fonts.** Latin-only font ≠ render Chinese/Japanese/Korean. Add `fonts-noto-cjk` for CJK-capable glyph coverage.
- **Long lines.** Translated text sometimes balloons. Wrap at ~40 chars in the pipeline before writing to the textfile, or use drawtext's `line_spacing` + manual newlines.
- **Timing.** drawtext reloads asynchronously. Expect 200-500ms lag between text update and frame showing new subtitle. For tight sync, write the file slightly ahead of TTS playout.

## Monitoring

`monitoring.tf` provisions a CloudWatch dashboard, log-based metric filters,
and alarms wired to an SNS topic. After `tofu apply`, the dashboard URL is in
the `dashboard_url` output.

### Alarms

| Name                           | Trigger                            | What it means                                                |
| ------------------------------ | ---------------------------------- | ------------------------------------------------------------ |
| `brivva-ecs-running-tasks-low` | `RunningTaskCount < 1` for 2m      | Task crashed or deploy rolling. Tunnel users see 530/1033.   |
| `brivva-ecs-cpu-high`          | CPU avg > 80% for 15m              | Saturated — bump `task_cpu` or scale out.                    |
| `brivva-ecs-memory-high`       | Mem avg > 85% for 15m              | OOM-kill imminent — bump `task_memory`.                      |
| `brivva-ffmpeg-crash-rate`     | > 3 ffmpeg crashes in 5m           | Destination RTMP unhealthy or CPU throttling.                |
| `brivva-ffmpeg-gave-up`        | Any "restart limit exceeded" in 1m | A stream is fully DOWN until the session restarts. **Page.** |

The crash + give-up alarms rely on metric filters that match the tracing JSON
`fields.message` field. If you rename a log line in `server-rs/**`, update the
filter pattern in `monitoring.tf` — the alarms silently report zero otherwise.

### Wiring alerts

```hcl
alarm_email = "oncall@example.com"
```

AWS sends a confirmation email on first `tofu apply`. Click the link before
alarms will actually deliver.

For Slack/PagerDuty, subscribe extra endpoints to the `alarm_topic_arn`
output (a Lambda → Slack webhook is the classic pattern).

Disable the whole block with `alarm_enabled = false` for dev stacks that
share an AWS account.

### Dashboard layout

- Row 1: ECS CPU/memory % (left) + running/desired task count (right)
- Row 2: ffmpeg crash/give-up counts (left) + session starts + WS auth
  rejections + workers status failures (right)
- Row 3: log widget showing recent WARN/ERROR lines via Logs Insights

Extend by editing the `widgets` list in `monitoring.tf`.

## State

State lives in S3 with DynamoDB locking:

- Bucket: `brivva-tf-state`
- Key: `brivva/terraform.tfstate`
- Lock table: `brivva-tf-locks`

Run OpenTofu through Infisical so sensitive `TF_VAR_*` values are injected at process runtime. The wrapper also exports AWS CLI `login` credentials into the OpenTofu process when needed:

```bash
infisical run --env=prod -- ./infra/tofu-infisical.sh plan
```

## Cloudflared

Sidecar auto-enabled iff both `tunnel_creds` AND `tunnel_id` are non-empty. Without them, the task has no ingress — add an ALB or populate the vars.

**Named tunnels are persistent.** Same `TUNNEL_CREDS` + `TUNNEL_ID` rejoin the same tunnel after every task restart. Hostname→tunnel route lives in Cloudflare DNS, not regenerated.

**Scale-to-zero limitation:** Cloudflare can't wake Fargate. If `desired_count=0` and a request lands, visitor gets `530/1033`. Livestream is stateful (long-lived WS, ffmpeg processes, SQLite), so stay at `desired_count=1`.

## Destroy

```fish
tofu destroy
```

Deletes ECR repos with all images and the Secrets Manager secret (zero recovery window). Only run when you mean it.
