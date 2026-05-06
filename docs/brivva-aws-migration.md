# Brivva AWS GPU Migration Runbook

Last updated: 2026-05-06

## Goal

Move Brivva media/server-rs GPU runtime into the Brivva-owned AWS account in `us-east-1`, with **zero idle GPU spend**. Keep GPU capacity at 0 by default; scale to 1 only for demos/stress tests.

## Account / region

- AWS account: `983601045027`
- AWS account name shown in console: `brivva`
- IAM user used locally: `typosbro@proton.me`
- Region: `us-east-1`
- Console URL: `https://983601045027.signin.aws.amazon.com/console`

Sanity check before any AWS command:

```bash
aws sts get-caller-identity
aws configure get region
```

Expected:

```text
Account = 983601045027
Region  = us-east-1
```

If account is anything else, stop.

## Billing / cost guardrails

- AWS Billing UI is restricted for this IAM user.
- Existing budget found: `My Monthly Cost Budget = $300/month`.
- Do not run GPU capacity unless quota is approved and demo/stress test is intentional.
- Current design keeps:
  - Auto Scaling Group desired capacity = `0`
  - ECS service desired count = `0`
  - no EC2 GPU instance running by default

Check current spend-risk state:

```bash
scripts/aws-gpu-scale.sh status
```

Expected idle state:

```text
ASG desired: 0
ASG instances: 0
ECS brivva desired/running: 0/0
ECS brivva-gpu desired/running: 0/0
```

## Current blocker

AWS GPU quota is pending.

Requested quotas in `us-east-1`:

- `Running On-Demand G and VT instances` → desired `8 vCPU`
  - Case ID: `177805550400721`
  - Request ID: `5c1cbfa19cac492ca9623188859eca69kxl3oe3E`
  - Status at last check: `CASE_OPENED`
- `All G and VT Spot Instance Requests` → desired `8 vCPU`
  - Case ID: `177805550400519`
  - Request ID: `c12926885cfb47c88f8caef7230c867bMU6nddQf`
  - Status at last check: `CASE_OPENED`

Check status:

```bash
aws service-quotas list-requested-service-quota-change-history-by-quota \
  --region us-east-1 \
  --service-code ec2 \
  --quota-code L-DB2E81BA \
  --query 'RequestedQuotas[0].{Desired:DesiredValue,Status:Status,CaseId:CaseId,Updated:LastUpdated}' \
  --output json

aws service-quotas list-requested-service-quota-change-history-by-quota \
  --region us-east-1 \
  --service-code ec2 \
  --quota-code L-3819A6DF \
  --query 'RequestedQuotas[0].{Desired:DesiredValue,Status:Status,CaseId:CaseId,Updated:LastUpdated}' \
  --output json
```

Why `vCPU` means GPU access:

- AWS GPU families are limited by vCPU quota.
- `g4dn.xlarge` = 4 vCPU + 1 NVIDIA T4 GPU.
- Quota `8 vCPU` permits up to two `g4dn.xlarge` instances, or one launch instance with headroom.

Current quota was `0`, so launching `g4dn.xlarge` is impossible until AWS approves.

## Prepared AWS resources

Terraform backend created in Brivva account:

- S3 bucket: `brivva-tf-state-983601045027`
- DynamoDB lock table: `brivva-tf-locks`

ECR repositories created:

- `brivva/server-rs`
- `brivva/ffmpeg-base`
- `brivva/ffmpeg-gpu-base`
- `brivva/server-build-base`
- `brivva/server-runtime-base`

Base/server images pushed:

- `983601045027.dkr.ecr.us-east-1.amazonaws.com/brivva/ffmpeg-gpu-base:7.1.1-nvenc`
- `983601045027.dkr.ecr.us-east-1.amazonaws.com/brivva/server-build-base:rust-1.88-slim-zigbuild-v1`
- `983601045027.dkr.ecr.us-east-1.amazonaws.com/brivva/server-runtime-base:bookworm-ffmpeg-7.1.1-nvenc-v1`
- `983601045027.dkr.ecr.us-east-1.amazonaws.com/brivva/server-rs:latest`
- `983601045027.dkr.ecr.us-east-1.amazonaws.com/brivva/server-rs:83265cecf46a26b8750be159f0938c9db56e8cf4`

Terraform scale-zero baseline applied:

- ECS cluster: `brivva`
- ECS services:
  - `brivva` desired `0`
  - `brivva-gpu` desired `0`
- Auto Scaling Group: `brivva-ecs-gpu`
  - min `0`
  - max `1`
  - desired `0`
- Launch template for `g4dn.xlarge`
- IAM roles/profile for ECS EC2 GPU + ECS task execution
- Security group: WebRTC UDP `40000-40100`, service TCP `3000` internal/VPC
- Secrets Manager: `brivva/env`
- CloudWatch log group: `/ecs/brivva`
- Log metric filters

No GPU/EC2 instance should exist while desired capacity is 0.

## Repo changes committed

Commit containing infra preparation:

```text
bfc9bbf chore(infra): prep scale-zero GPU AWS stack
```

Previous production streaming hardening commit:

```text
83265ce fix(streaming): harden prod YouTube RTMP
```

Both pushed to GitLab and GitHub `main`.

## Important files

- `deploy.sh` — builds/pushes server-rs/base images; can deploy ECS task defs after service exists.
- `infra/versions.tf` — OpenTofu/Terraform backend now points at `brivva-tf-state-983601045027`.
- `infra/variables.tf` — GPU desired defaults set to zero.
- `infra/main.tf` — primary ECS service desired count controlled by `gpu_service_desired_count`.
- `scripts/aws-gpu-scale.sh` — start/stop/status helper for GPU stack.
- `scripts/prod-youtube-oauth-e2e.mjs` — production YouTube OAuth E2E validation runner.
- `docs/2026.05.06-report.md` — production streaming incident/report context.

## Start/stop GPU stack

Status:

```bash
scripts/aws-gpu-scale.sh status
```

Start when quota is approved and a demo/stress run is intended:

```bash
scripts/aws-gpu-scale.sh start
```

Stop immediately after testing/demo:

```bash
scripts/aws-gpu-scale.sh stop
```

Expected start time:

- first start: ~5–10 min
- later starts: ~2–5 min

Start sequence:

1. ASG desired capacity → 1
2. EC2 GPU instance boots
3. ECS agent registers instance
4. ECS service desired count → 1
5. task pulls `server-rs:latest`
6. server-rs + cloudflared start

## Deploy image without starting GPU

Build/push all base images only:

```bash
AWS_REGION=us-east-1 AWS_ACCOUNT=983601045027 \
  ./deploy.sh --bases-only --build-ffmpeg-gpu-base --build-server-bases
```

Build/push server image only, no ECS deploy:

```bash
AWS_REGION=us-east-1 AWS_ACCOUNT=983601045027 \
  ./deploy.sh --build-only
```

Deploy/update ECS task definition after stack exists:

```bash
AWS_REGION=us-east-1 AWS_ACCOUNT=983601045027 ./deploy.sh
```

Note: full `deploy.sh` updates ECS service. If service desired count is 0, it should not start GPU by itself, but use caution and verify status after.

## Terraform / OpenTofu usage

Use OpenTofu through Infisical so secrets are injected without writing tfvars files:

```bash
infisical run --env=prod -- ./infra/tofu-infisical.sh plan \
  -var gpu_desired_capacity=0 \
  -var gpu_service_desired_count=0 \
  -var gpu_rehearsal_desired_count=0
```

Apply scale-zero baseline:

```bash
infisical run --env=prod -- ./infra/tofu-infisical.sh apply \
  -var gpu_desired_capacity=0 \
  -var gpu_service_desired_count=0 \
  -var gpu_rehearsal_desired_count=0
```

To intentionally scale via Terraform for demo:

```bash
infisical run --env=prod -- ./infra/tofu-infisical.sh apply \
  -var gpu_desired_capacity=1 \
  -var gpu_service_desired_count=1 \
  -var gpu_rehearsal_desired_count=0
```

Prefer `scripts/aws-gpu-scale.sh start/stop` for daily demo operations because it is faster and less likely to touch unrelated infra.

## Production validation after start

After `scripts/aws-gpu-scale.sh start`, verify:

```bash
scripts/aws-gpu-scale.sh status
aws logs tail /ecs/brivva --region us-east-1 --follow
```

Run production YouTube OAuth E2E:

```bash
E2E_RECORD_SECONDS=300 \
E2E_AUDIO_FILE=tmp/prod-e2e-speech.wav \
node scripts/prod-youtube-oauth-e2e.mjs
```

Expected validation:

- session status becomes `ended`
- YouTube broadcast completes and becomes VOD, not stuck live
- `unbillable_windows` remains empty for healthy run
- no `Media connection problem`
- no provider health errors
- no `output.degraded` / RTMP KO
- Korean output minutes billed

Useful summary endpoint:

```bash
curl -s 'https://brivva-api.milliytechnology.workers.dev/api/sessions/<SESSION_ID>/summary' | jq .
```

## Stress-test plan after quota approval

Minimum stages:

1. Smoke: one YouTube stream, one target language (`ko`), 5 min.
2. Demo shape: two translated languages + one passthrough, 10–15 min.
3. Longer burn: 30 min.
4. Stop/cleanup: verify ECS stop, session finalization, YouTube VOD completion.

For 2 translated languages + passthrough on `g4dn.xlarge`:

- T4 NVENC should handle launch shape.
- Watch FFmpeg progress, output health, TTS queue depth, Soniox reconnects, and WebRTC quality.
- Keep fake media fixture quality in mind: low-res/15fps fixtures trigger media-quality warnings that are not necessarily production failures.

## Known caveats

- AWS Support API `describe-cases` requires Premium Support, so case details are checked through Service Quotas, not Support API.
- Current AWS account is new compared to older infra. Old account `132593557399` appeared in prior artifacts/deploys; do not deploy there unless explicitly intended.
- Cloudflared is enabled because Infisical provided tunnel credentials. If domain routing still points to old environment, update Cloudflare tunnel/DNS before final production cutover.
- First GPU start may reveal IAM, tunnel, or ECS registration drift. This is normal first-smoke work, not a design blocker.
- Do not paste AWS console passwords, callback URLs, OAuth codes, API keys, RTMP stream keys, or secrets into chat or docs.

## Immediate next action

Wait for AWS quota approval. Then run:

```bash
scripts/aws-gpu-scale.sh start
```

After validation/stress/demo:

```bash
scripts/aws-gpu-scale.sh stop
```
