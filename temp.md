# RS-007 AWS Soak — Concise Plan

Goal: prove AWS production media path before paid live.

## Launch Shape

- Region: `us-east-1` unless latency tests justify another region.
- Compute: ECS on EC2 GPU.
- `brivva-gpu` private rehearsal = GPU/image/fake-sink proof only unless NAT/egress + operator access added.
- Real-platform proof = primary `brivva` EC2_GPU cutover or rehearsal service with real egress/access.
- First GPU: `g4dn.xlarge`; upgrade if launch fanout falls below realtime.
- Encoder: `nvenc`.
- Output: H.264 `720x1280` portrait, 1s keyframes, ~2.5 Mbps.
- Input cap: 1080p30. 4K input must downsample; 4K output disabled unless separately proven.
- Platforms: launch mix: YouTube + Grip + TikTok/generic RTMP if used.
- Languages: launch languages + pass/source.

## Hard Rules

- Fake sink alone cannot close RS-007.
- Do not mutate shared prod `brivva/env` for bad-provider drills.
- Do not rely on mutable `latest`; prove SHA tag/digest actually used by task.
- Do not assume ECS Exec; current infra does not enable it.
- Private no-public-IP GPU needs:
  - AWS VPC endpoints or NAT for ECS/ECR/Secrets/CloudWatch;
  - NAT/public egress for Soniox, ElevenLabs, YouTube/Grip/TikTok;
  - VPN/bastion/SSM tunnel or primary route for operator/browser access.

## Artifacts Per Run

Save under `tmp/aws-soak-runs/YYYYMMDD-HHMM-profile/`:

- `manifest.md`: commit, image digest, task def, service, network mode, instance ID/type/AZ, FFmpeg/NVENC/font proof, fixture checksum, platforms/langs, key freshness, UTC/KST times.
- `terraform-plan.txt`
- ECS describe outputs
- CloudWatch logs
- `smoke.log`, `soak.log`, `provider-drills.log`
- `platform-visible-live.md`
- `billing-check.md`
- `verdict.md`

## Pass Gates

### Dry Run

Must prove:

- GPU host registered; task stable 5+ min.
- Logs reach CloudWatch.
- Secrets present, not printed.
- Workers metrics endpoint accepts session metrics.
- FFmpeg has `rtmp`, `rtmps`, OpenSSL, `drawtext`, `libharfbuzz`, `h264_nvenc`, AAC.
- CJK font renders, no tofu boxes.
- NVENC works inside ECS task.
- Network mode matches test goal.

### Exact 30–60 Min Soak

Must show:

- no FFmpeg crash/restart;
- no sustained below-realtime encode;
- no normal-run media stale drops;
- `tts segment queue overflow=0`;
- `hard_recovery=0` unless allowed;
- TTS buffer drains; speed returns to `1.00`;
- source/pass and translated outputs visible live;
- JA/ZH/KO or launch-language audio intelligible;
- every platform dashboard healthy;
- Workers metrics/session logs received;
- unbillable failure windows excluded from billing.

### Failure Drills

After clean soak:

- bad Soniox key via isolated task/secret only;
- bad ElevenLabs key/TTS failure via isolated task/secret only;
- one bad RTMP destination with good siblings;
- bad platform key for one output;
- operator stop/restart or documented manual ECS workaround.

Expected: affected lane fails visibly + non-billable; source/pass/siblings stay live.

## Execution Order

1. Pick proof mode:
   - private fake-sink only;
   - private real-platform with NAT/access;
   - primary EC2_GPU cutover.
2. Replace/restore missing GPU helper scripts or use region-explicit AWS CLI.
3. Add/confirm startup self-check logs for FFmpeg/NVENC/fonts.
4. Add isolated rehearsal secret/task override path for bad-provider drills.
5. Build/push GPU image; record immutable SHA/digest.
6. Scale `brivva-gpu` only after network mode ready.
7. Run dry-run capability probe.
8. Run fake/local sink smoke.
9. Run YouTube smoke only from real-platform-capable mode.
10. Run Grip smoke with fresh one-shot key if launch uses Grip.
11. Run 30–60 min exact launch soak.
12. Run failure drills.
13. Verify billing.
14. Update `docs/rust-stress-test/tracker.md`.

## Closure

Move RS-007 to `patched` only after dry-run, real-platform smoke, exact soak, failure drills/fallbacks, billing verification, artifacts saved, and tracker updated.

If soak passes but drills incomplete: set RS-007 to `watch`, not `patched`.
