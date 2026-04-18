# infra — Terraform (AWS us-east-1)

Reproducible AWS infra for Brivva backend. Region fixed to `us-east-1` for proximity to Soniox + ElevenLabs.

## What it provisions

| Resource | Name | Notes |
|---|---|---|
| ECR repo | `brivva/server-rs` | Keep last 10 images (lifecycle policy) |
| Secrets Manager secret | `brivva/env` | JSON — keys below |
| IAM role | `brivva-ecs-execution` | ECS task exec + `secretsmanager:GetSecretValue` on `brivva/env` |
| CloudWatch log group | `/ecs/brivva` | 7d retention |
| ECS cluster | `brivva` | Fargate-only |
| ECS task def | `brivva` | 2 vCPU / 4 GB, server-rs + optional cloudflared sidecar |
| ECS service | `brivva` | 1 task, public IP in default VPC |
| Security group | `brivva-task` | Egress-only (cloudflared handles ingress) |

Secret JSON keys: `SONIOX_API_KEY`, `ELEVENLABS_API_KEY`, `GOOGLE_CLIENT_ID`, `GOOGLE_CLIENT_SECRET`, `TUNNEL_CREDS`.

## First apply

```fish
# 1. Install OpenTofu (or HashiCorp Terraform)
brew install opentofu

# 2. Configure AWS creds for an account with admin-ish perms
aws configure   # or: set -x AWS_PROFILE brivva

# 3. Load secrets from ../.env.local → terraform.tfvars (gitignored)
cd infra
./load-env.sh

# 4. Init + apply
tofu init
tofu apply
```

Outputs include `ecr_server_url`, `account_id`, etc.

## Redeploy (after code change)

Terraform does NOT rebuild images. Flow:

```fish
cd ..
./deploy.sh   # build → push :latest → force new ECS deployment
```

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

## Rotate secrets

Edit `../.env.local`, then:

```fish
cd infra
./load-env.sh
tofu apply          # updates secret value
cd ..
./deploy.sh --skip-build   # restart tasks to pick up new values
```

Tasks fetch secrets at container start — they don't hot-reload.

## Sizing

**Current default: 2 vCPU / 4 GB.**

Rough budget per running stream:

| Work | CPU per process | RAM per process |
|---|---|---|
| Audio-only encode (AAC) | ~5% of 1 core | 50 MB |
| Video passthrough (`-c:v copy`) | ~5% of 1 core | 50 MB |
| libx264 720p30 transcode | ~50% of 1 core | 200 MB |
| libx264 1080p30 transcode | ~80% of 1 core | 400 MB |
| + drawtext subtitle overlay | +5-10% | negligible |

**One stream = one ffmpeg process per RTMP destination** (source + K translated languages = 1+K processes).

### Can a 2 vCPU / 4 GB task handle it?

| Scenario | Fit |
|---|---|
| 1 stream, 1080p transcode, 2 translations + burn-in subs | ✅ comfortable (~2.5 cores worth spread across 2 actual — Fargate lets you burst briefly, sustained 2-lang OK, 3-lang tight) |
| 1 stream, H.264 passthrough from browser, 4 translations | ✅ lots of headroom |
| 2 concurrent host sessions, 1080p transcode, 2 translations each | ❌ needs 4 vCPU / 8 GB, or horizontal scale |

### Tune via tfvars

```hcl
# infra/terraform.tfvars
task_cpu    = "4096"   # 4 vCPU
task_memory = "8192"   # 8 GB
```

Valid Fargate combos: see [AWS docs](https://docs.aws.amazon.com/AmazonECS/latest/developerguide/task-cpu-memory-error.html). Common pairs: 1024/2048, 2048/4096, 4096/8192, 8192/16384.

Apply changes: `tofu apply`, then `./deploy.sh --skip-build` to roll tasks.

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
- >2 concurrent hosts routinely

## Subtitles architecture

Live subtitle support is uneven across platforms:

| Platform | Text caption ingest | How |
|---|---|---|
| YouTube Live | ✅ | HTTP POST per-broadcast caption URL |
| Facebook Live | ✅ | Graph API captions endpoint |
| Twitch | ⚠️ | CEA-608/708 embedded as H.264 SEI |
| Instagram / TikTok / Kuaishou / Bilibili / custom RTMP | ❌ | — |

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

## State

Local backend for now (`terraform.tfstate` in this dir, gitignored). Migrate to S3 + DynamoDB lock before team use.

## Cloudflared

Sidecar auto-enabled iff both `tunnel_creds` AND `tunnel_id` are non-empty. Without them, the task has no ingress — add an ALB or populate the vars.

**Named tunnels are persistent.** Same `TUNNEL_CREDS` + `TUNNEL_ID` rejoin the same tunnel after every task restart. Hostname→tunnel route lives in Cloudflare DNS, not regenerated.

**Scale-to-zero limitation:** Cloudflare can't wake Fargate. If `desired_count=0` and a request lands, visitor gets `530/1033`. Livestream is stateful (long-lived WS, ffmpeg processes, SQLite), so stay at `desired_count=1`.

## Destroy

```fish
tofu destroy
```

Deletes ECR repos with all images and the Secrets Manager secret (zero recovery window). Only run when you mean it.
