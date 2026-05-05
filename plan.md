# RS-007 AWS Launch-Profile Soak Plan

Status: draft
Owner: Aziz / Brivva engineering
Tracker item: `docs/rust-stress-test/tracker.md` → RS-007
Goal: prove the exact AWS production path before any paid live-commerce show.

## 1. Why This Exists

Local Rust stress tests proved important media behavior, but RS-007 remains open because AWS can fail in ways local runs cannot expose:

- ECS task placement / service rollout problems.
- EC2 GPU host capacity or quota failure.
- NVIDIA driver / NVENC missing or unusable inside the task.
- FFmpeg build drift: missing native RTMP/RTMPS, OpenSSL, `libharfbuzz`, `drawtext`, `h264_nvenc`.
- CJK font/subtitle rendering failure.
- Secrets Manager / IAM / Infisical secret mismatch.
- CloudWatch log shipping gaps.
- Workers internal metrics/session-log failures.
- AWS egress too weak for real RTMP fanout.
- Platform acceptance mismatch: AWS pushes bytes, but YouTube/Grip/TikTok does not show a visible healthy live.

This plan treats AWS as unproven until the exact launch task/image/secrets/profile completes dry-run, soak, and failure drills.

## 2. Launch Profile To Prove

Default launch profile unless product explicitly changes it:

| Area | Launch value |
| --- | --- |
| AWS region | `us-east-1` |
| Compute path | ECS on EC2 GPU. Private `brivva-gpu` rehearsal may prove image/GPU/fake-sink only until egress + operator access are explicit; real-platform proof uses either primary `brivva` EC2_GPU cutover or a rehearsal service with NAT/public egress + private operator access. |
| Instance target | `g4dn.xlarge` first capability target; upgrade to larger NVIDIA host if exact launch fanout/CPU/STT/TTS load falls below realtime. |
| Encoder | `nvenc` |
| Output profile | mobile-first RTMP: H.264, `720x1280` portrait, 1s keyframes, ~2.5Mbps video |
| Input cap | accept host up to 1080p30 launch tier; 4K host must downsample to 1080p/portrait output |
| 4K output | disabled unless separately sold and separately passed on exact AWS hardware |
| Source language | launch rehearsal language, normally `ko` or `en` based on show host |
| Target languages | all launch languages, e.g. `ko`, `ja`, `zh`, plus pass/source output |
| Platforms | all launch platforms with real one-shot/current keys: YouTube + Grip + TikTok/generic RTMP if used |
| Network path | no-public-IP GPU needs VPC endpoints for AWS control plane and NAT/egress for Soniox, ElevenLabs, YouTube/Grip/TikTok; browser/operator access needs VPN/bastion/SSM tunnel or primary public route. |
| Duration | 30-60 min exact-profile soak after shorter capability checks |
| Billing | provider failure windows persisted and excluded from billable usage |

Hard rule: do not mark RS-007 closed from fake sink only. Fake sink is useful for GPU/image capability, but production proof requires at least one real platform and the launch platform mix if available. Current `brivva-gpu` Terraform is private/shadow/fake-sink by default, so it is not a full paid-live proof until network/egress/access mode is changed intentionally.

## 3. Required Artifacts

Create one run folder per AWS rehearsal:

```text
tmp/aws-soak-runs/YYYYMMDD-HHMM-<profile>/
  manifest.md
  terraform-plan.txt
  ecs-describe-services.json
  ecs-describe-tasks.json
  ecs-container-instance.json
  cloudwatch-server-rs.log
  cloudwatch-gpu-server-rs.log
  smoke.log
  soak.log
  provider-drills.log
  platform-visible-live.md
  billing-check.md
  verdict.md
```

`manifest.md` must record:

- git commit SHA;
- Docker image digest actually used by the task, not only mutable `latest`;
- Terraform/OpenTofu var set;
- ECS task definition ARN;
- ECS service name (`brivva-gpu` rehearsal or primary `brivva`);
- network mode used for the run: fake-sink private rehearsal, private rehearsal with NAT/operator access, or primary EC2_GPU cutover;
- EC2 instance ID/type/AZ;
- FFmpeg version/config output;
- `ffmpeg -protocols` proof for `rtmp`/`rtmps` and `ffmpeg -version` proof for OpenSSL;
- `ffmpeg -encoders | rg h264_nvenc` result;
- font list / CJK font proof;
- exact source MP4 fixture path/checksum;
- exact output platforms/languages;
- stream keys age/freshness note, especially Grip one-shot key;
- start/end timestamps in UTC and KST.

## 4. Pass / Fail Gates

### 4.1 Dry-Run Capability Gate

Pass only if AWS task proves:

- ECS GPU host registered in cluster.
- Task is `RUNNING` and stable for at least 5 minutes before media starts.
- Network path matches test goal:
  - fake-sink/private rehearsal: VPC endpoints or NAT are enough for ECS/ECR/Secrets/CloudWatch;
  - real platform: NAT/public egress reaches Soniox, ElevenLabs, YouTube/Grip/TikTok, and operator/browser can reach the task through VPN/bastion/SSM tunnel or primary route.
- Container logs reach CloudWatch.
- Secrets are present without printing values.
- Workers internal metrics endpoint accepts session metrics.
- FFmpeg supports required launch features:
  - `rtmp` and `rtmps` protocols;
  - OpenSSL/native RTMPS path;
  - `drawtext` filter;
  - `libharfbuzz`/font shaping path;
  - `h264_nvenc` encoder;
  - AAC audio encoder.
- Runtime fonts include CJK fonts and subtitle render does not produce tofu boxes.
- NVENC encode can start from inside ECS task, not just on host.

Fail fast if any of these are missing. Do not continue to real platform soak.

### 4.2 Exact-Profile Soak Gate

Pass only if 30-60 min AWS launch-profile run has:

- no FFmpeg publisher crash/restart;
- no sustained below-realtime encode after warmup;
- no `video_stale_chunks_dropped` in normal healthy run;
- no `host_audio_stale_chunks_dropped` in normal healthy run;
- no `ready_host_bytes_dropped` in normal healthy run;
- `tts segment queue overflow=0` for normal healthy run;
- `final_policy="hard_recovery"=0` unless the scenario explicitly allows it;
- `tts_buffered_bytes` drains and does not grow forever;
- `tts_playback_speed` returns to `1.00` after catch-up;
- source/pass output stays live;
- translated audio is human-intelligible for JA/ZH/KO or launch languages;
- every intended platform dashboard shows visible healthy live, not only RTMP bytes sent;
- CloudWatch logs contain enough per-output/platform/lang IDs to debug failures;
- Workers receives health/metrics/session logs;
- billing summary excludes any intentionally injected unbillable failure windows.

### 4.3 Failure Drill Gate

Before paid production, AWS path must pass:

- bad Soniox key injected only into an isolated rehearsal task/override/secret, never the shared production `brivva/env` secret: source/pass continues; translation lanes fail visibly; translated billing pauses;
- bad ElevenLabs key or forced TTS failure injected only into an isolated rehearsal task/override/secret: source/pass and captions continue where applicable; TTS billing pauses;
- one bad RTMP destination mixed with good destinations: bad output fails, siblings stay live;
- platform key rejection/bad stream key: affected output fails and is non-billable, siblings stay live;
- operator stop/restart one output if the control exists; otherwise document manual ECS/operator workaround.

## 5. Phase Plan

## Phase A — Repo / Config Preflight

1. Confirm tracker and docs are current:
   - `docs/rust-stress-test/tracker.md`
   - `docs/rust-stress-test/production_readiness.md`
   - `docs/rust-stress-test/provider_failure_billing.md`
   - `docs/rust-stress-test/scenarios.md`
   - `docs/rust-stress-test/signals.md`
2. Confirm working tree state and commit SHA.
3. Confirm `deploy.sh --gpu` path still builds GPU runtime image.
4. Confirm helper scripts referenced by `infra/README.md` actually exist. Current repo lacks most GPU helper scripts (`run-ecs-gpu-rehearsal.sh`, `ecs-gpu-endpoint.sh`, `check-aws-gpu-quota.sh`, etc.). If missing, either:
   - restore/add helper scripts, or
   - use AWS CLI equivalents and capture commands in `manifest.md`.
5. Confirm launch caps:
   - `BRIVVA_VIDEO_ENCODER=nvenc` on GPU task;
   - `BRIVVA_VIDEO_MAX_WIDTH=1920`;
   - `BRIVVA_VIDEO_MAX_HEIGHT=1080`;
   - `BRIVVA_VIDEO_MAX_FPS=30`;
   - RTMP output remains mobile-first portrait unless show explicitly requires source layout.
6. Confirm 4K is product-disabled unless exact AWS 4K run is added.
7. Decide network mode before touching AWS capacity:
   - private fake-sink only: allowed for GPU/image capability;
   - private real-platform: requires NAT/egress plus VPN/bastion/SSM/operator route;
   - primary EC2_GPU cutover: higher blast radius, but proves real public path.
8. Set `AWS_REGION=us-east-1`; every AWS CLI command in artifacts must show region explicitly or inherit this exported env.

Output: `manifest.md` started.

## Phase B — AWS Account / Quota / Terraform Preflight

1. Confirm AWS identity and region (`AWS_REGION=us-east-1`).
2. Confirm EC2 GPU quota supports at least one `g4dn.xlarge`:
   - service quota for running on-demand G/VT instances must cover 4 vCPU.
3. Confirm target AZ has `g4dn.xlarge` capacity. Keep `us-east-1e` excluded per `infra/variables.tf`.
4. Run OpenTofu plan for rehearsal mode first:

```bash
infisical run --env=prod -- ./infra/tofu-infisical.sh plan \
  -var='gpu_rehearsal_service_enabled=true' \
  -var='gpu_rehearsal_desired_count=0' \
  -var='gpu_desired_capacity=0'
```

5. Current GPU launch template uses no public IP. For private/no-public-IP GPU rehearsal, verify VPC endpoints and route table IDs intentionally:

```bash
infisical run --env=prod -- ./infra/tofu-infisical.sh plan \
  -var='gpu_rehearsal_service_enabled=true' \
  -var='gpu_rehearsal_desired_count=0' \
  -var='gpu_desired_capacity=0' \
  -var='gpu_private_endpoints_enabled=true' \
  -var='gpu_private_endpoint_route_table_ids=["rtb-..."]'
```

6. For any real-platform run from private GPU, confirm NAT/egress path exists for Soniox, ElevenLabs, YouTube/Grip/TikTok. VPC endpoints alone cover AWS control-plane pulls/logs/secrets, not external provider/platform traffic.
7. Save plan output to run folder.

Pass: plan only touches expected ECS/GPU/rehearsal resources and network changes match chosen mode.
Fail: unexpected primary service replacement, route-table blast radius, accidental public endpoint exposure, missing external egress for real-platform runs, or secret drift.

## Phase C — Build And Push Exact GPU Image

1. Build/push GPU bases if Dockerfile or FFmpeg version changed:

```bash
./deploy.sh --gpu --build-ffmpeg-gpu-base --build-server-bases --bases-only
```

2. Build/push server GPU image without switching primary service:

```bash
./deploy.sh --gpu --build-only
```

3. Resolve and record immutable image digest. Prefer the SHA tag from `deploy.sh`; do not rely on mutable `latest` as proof:

```bash
aws ecr describe-images \
  --region "$AWS_REGION" \
  --repository-name brivva/server-rs \
  --image-ids imageTag="$(git rev-parse HEAD)" \
  --query 'imageDetails[0].imageDigest' \
  --output text
```

4. Pin the rehearsal/soak task definition to the SHA tag or image digest, or record ECS/container metadata proving the pulled digest exactly matches the manifest.

Pass: image exists in ECR, digest is recorded, and task image proof is immutable.

## Phase D — Create Rehearsal Capacity At Zero Blast Radius

Prefer parallel `brivva-gpu` rehearsal service over primary cutover for GPU/image/fake-sink proof. Do not treat it as real-platform proof unless NAT/external egress and operator/browser access are intentionally added. Otherwise use a controlled primary `brivva` EC2_GPU cutover for real-platform smoke/soak.

1. Apply rehearsal service with zero desired count:

```bash
infisical run --env=prod -- ./infra/tofu-infisical.sh apply \
  -var='gpu_rehearsal_service_enabled=true' \
  -var='gpu_rehearsal_desired_count=0' \
  -var='gpu_desired_capacity=0'
```

2. Scale one GPU host + one rehearsal task only when ready to burn AWS cost and after private endpoint/NAT/access mode is decided:

```bash
infisical run --env=prod -- ./infra/tofu-infisical.sh apply \
  -var='gpu_rehearsal_service_enabled=true' \
  -var='gpu_rehearsal_desired_count=1' \
  -var='gpu_desired_capacity=1' \
  -var='gpu_instance_type=g4dn.xlarge'
```

3. Watch ECS service events until stable.
4. Save region-explicit outputs:
   - `aws ecs describe-services --region "$AWS_REGION" --cluster brivva --services brivva-gpu`
   - `aws ecs list-tasks --region "$AWS_REGION" --cluster brivva --service-name brivva-gpu`
   - `aws ecs describe-tasks --region "$AWS_REGION" --cluster brivva --tasks ...`
   - `aws ecs describe-container-instances --region "$AWS_REGION" ...`
   - CloudWatch startup logs.

Pass: one ECS GPU container instance registered, task running, logs flowing, and chosen network mode is proven.
Fail: task placement error, capacity unavailable, host cannot pull image, secrets unavailable, no CloudWatch logs, or private task cannot reach required external providers/platforms for a real-platform run.

## Phase E — Runtime Capability Probe Inside AWS Task

Need a way to execute or embed these probes. Options:

- startup self-check logs emitted by `server-rs` (preferred, works without shell access);
- one-shot diagnostic container/task using the same runtime image;
- temporary diagnostic endpoint/route guarded by internal auth;
- ECS Exec only after Terraform explicitly enables it and IAM/task-role/session-manager permissions are verified. Current infra should not assume ECS Exec works.

Required probe commands or equivalent startup logs:

```bash
ffmpeg -version
ffmpeg -hide_banner -protocols
ffmpeg -hide_banner -filters
ffmpeg -hide_banner -encoders
ffprobe -version
fc-match "Noto Sans CJK KR"
```

Required assertions:

```text
ffmpeg config includes openssl/native-rtmps path
protocols include rtmp and rtmps
filters include drawtext
encoders include h264_nvenc
gpu runtime can load NVIDIA encode libs
CJK font resolves to Noto/DejaVu fallback, not missing
```

NVENC smoke command if shell access exists:

```bash
ffmpeg -hide_banner -f lavfi -i testsrc2=size=1280x720:rate=30 \
  -t 10 -c:v h264_nvenc -f null -
```

Subtitle/font smoke:

```bash
ffmpeg -hide_banner -f lavfi -i color=black:s=720x1280:r=30 \
  -t 5 -vf "drawtext=font='Noto Sans CJK KR':text='한국어 日本語 中文':x=40:y=80:fontsize=48:fontcolor=white" \
  -c:v h264_nvenc -f null -
```

Pass: both commands run near realtime and return 0.
Fail: stop; fix image/host/driver/fonts before any platform run.

## Phase F — AWS Fake-Sink / Local-Sink Media Smoke

Purpose: separate AWS encode/media bugs from platform keys/network dashboards.

Run one short server-rs media smoke against fake/local RTMP sink if supported. This can use private `brivva-gpu` without external platform egress. If no AWS-side stress entrypoint exists yet, patch one before continuing.

Target smoke:

- duration: 5-10 min;
- encoder: `nvenc`;
- input: known H.264 1080p30 fixture;
- outputs: pass/source + one translated language;
- no real customer platform yet.

Pass signals:

- speed near `1.0x` after warmup;
- no media stale drops;
- no FFmpeg restart;
- TTS drains;
- CloudWatch logs enough metrics.

Fail: fix Rust/FFmpeg/AWS before consuming real platform keys.

## Phase G — Single Real Platform Smoke

Purpose: prove AWS egress + real RTMP + visible live.

Precondition: the task must have external egress to providers/platforms and operator/browser access to start/watch the run. Private `brivva-gpu` fake-sink mode does not satisfy this by itself; add NAT/access or use controlled primary `brivva` EC2_GPU cutover.

1. Use one YouTube test event first.
2. Then use Grip smoke with a fresh one-shot Grip key if Grip is part of launch.
3. Duration: 10-15 min each.
4. Use launch output profile: portrait `720x1280`, H.264, 1s keyframes, <3Mbps for Grip.

Platform proof checklist:

- Rust logs show output started.
- FFmpeg logs show increasing bytes/chunks and speed near realtime.
- Platform dashboard shows video and audio healthy.
- Human watches output for subtitles/audio sync.
- No crash/restart.
- Workers metrics/session logs arrive.

Grip-specific:

- fresh one-shot `STREAM_URL_GRIP` + `STREAM_KEY_GRIP` only;
- verify Grip Studio receives `720x1280`, H.264, audio, increasing chunks;
- do not reuse key for second run.

Pass: AWS is real-platform capable for one platform.
Fail: classify as AWS egress, FFmpeg/RTMP, platform key, or platform visible-live issue.

## Phase H — Exact Launch-Profile Soak

Run the real launch shape:

- AWS ECS GPU task;
- `nvenc`;
- real launch source fixture or host feed equivalent;
- all launch languages;
- all launch platform outputs possible with fresh keys;
- 30-60 min duration;
- no manual restarts during run unless testing operator recovery.

Suggested run name:

```text
aws_launch_profile_1080p30_all_outputs_YYYYMMDD_HHMM
```

During soak, watch:

```bash
# logs/signals copied locally after or streamed during run
rg "test result|mp4 fanout smoke finished|session" tmp/aws-soak-runs/**/soak.log
rg "tts segment queue overflow|tts_playback_speed|tts_catchup_active" tmp/aws-soak-runs/**/soak.log
rg "drop_frames=|speed=|encode below realtime" tmp/aws-soak-runs/**/soak.log
rg "video_stale_chunks_dropped|host_audio_stale_chunks_dropped|ready_host_bytes_dropped" tmp/aws-soak-runs/**/soak.log
rg "ffmpeg rtmp process crashed|publisher restart|provider_health|billable=false" tmp/aws-soak-runs/**/soak.log
```

Human checks every platform:

- original audio/video live;
- translated audio intelligible;
- subtitles readable, no CJK tofu boxes;
- platform dashboard health green/stable;
- no growing latency that makes commerce interaction unusable.

Pass: all pass/fail gates in section 4.2.
Fail: record exact first-bad timestamp and classify.

## Phase I — AWS Provider/Platform Failure Drills

Run after one clean exact-profile soak, not before.

### I.1 Bad Soniox Key

- Deploy/override task with intentionally bad `SONIOX_API_KEY` in an isolated rehearsal task/secret only.
- Do **not** mutate shared production `brivva/env` or Infisical prod secret values; primary `brivva` reads the same secret at task start.
- Run 5-10 min single platform.
- Expected:
  - source/pass stays live;
  - translation lanes fail visibly;
  - provider health event has `provider=soniox`, `billable=false`;
  - Workers failure window persisted;
  - billing excludes affected translated minutes.

### I.2 Bad ElevenLabs Key / TTS Failure

- Use intentionally bad `ELEVENLABS_API_KEY` via isolated rehearsal task/secret, or existing test scenario if available.
- Do **not** mutate shared production `brivva/env` or Infisical prod secret values.
- Expected:
  - source/pass stays live;
  - captions/translation continue if Soniox is healthy;
  - TTS lane fails visibly;
  - provider health event has `provider=elevenlabs`, `billable=false`;
  - billing excludes affected TTS/audio minutes.

### I.3 One Bad Destination

- Configure all good launch outputs plus one dead RTMP URL.
- Expected:
  - bad destination emits failed health;
  - at least one sibling output reaches visible live;
  - no global stream crash;
  - only bad platform/output is non-billable.

### I.4 Bad Platform Key

- Use bad YouTube/RTMP key in one output only.
- Expected:
  - that output fails/restarts then marks failed;
  - siblings remain live;
  - billing excludes failed output window.

Pass: failure windows visible in logs/frontend/Workers/billing.
Fail: production remains operator-supervised and RS-007 stays open.

## Phase J — Billing Verification

For every clean soak and every failure drill, record:

- session ID;
- output IDs;
- provider health windows;
- Workers `/internal/sessions/:id/metrics` acceptance;
- usage/summary output;
- unbillable audit buckets.

Minimum billing assertions:

- clean soak billable minutes roughly match delivered healthy windows;
- bad Soniox creates unbillable source/lang translation window;
- bad ElevenLabs creates unbillable TTS/lang window;
- bad RTMP/platform creates unbillable output/platform window;
- sibling healthy outputs remain billable.

If platform SKU mapping is not final, write product caveat in `billing-check.md` and keep RS-006/RS-007 watch/open as appropriate.

## Phase K — Rollback / Cost Kill Switch

Before scaling GPU up, prepare exact rollback commands.

Rehearsal scale-down:

```bash
infisical run --env=prod -- ./infra/tofu-infisical.sh apply \
  -var='gpu_rehearsal_service_enabled=true' \
  -var='gpu_rehearsal_desired_count=0' \
  -var='gpu_desired_capacity=0'
```

Primary fallback if direct cutover was attempted:

```bash
infisical run --env=prod -- ./infra/tofu-infisical.sh apply \
  -var='ecs_launch_type=FARGATE' \
  -var='gpu_desired_capacity=0'
```

Post-run cost check:

```bash
aws autoscaling describe-auto-scaling-groups \
  --region "$AWS_REGION" \
  --auto-scaling-group-names brivva-ecs-gpu

aws ecs describe-services \
  --region "$AWS_REGION" \
  --cluster brivva \
  --services brivva-gpu brivva
```

Pass: GPU ASG desired/running capacity returns to 0 after rehearsal unless intentionally left for launch. If primary EC2_GPU cutover was used, verify service is back on Fargate or intentionally left on GPU with operator approval.

## 6. Known Gaps To Patch If Missing

These are blockers if the repo cannot currently perform the AWS soak directly:

0. **Network/egress/access mode**
   - Current GPU launch template has no public IP and `brivva-gpu` is documented private/shadow/fake-sink.
   - For real-platform smoke, add NAT/external egress plus operator/browser access, or use controlled primary EC2_GPU cutover.
   - Do not start YouTube/Grip/TikTok proof from private fake-sink mode and call it production-equivalent.

1. **AWS-side stress entrypoint**
   - Current `scripts/run-rust-stress-tests.sh` is local-oriented.
   - Need a way to run equivalent MP4 smoke against ECS task or start a rehearsal session through the real API/frontend.

2. **Runtime self-check endpoint/log**
   - Add startup self-check logs for FFmpeg protocols/filters/encoders/fonts/NVENC.
   - Better than depending on ECS Exec; current Terraform does not enable ECS Exec by default.

3. **CloudWatch log export helper**
   - Script should pull server logs for a time range into run folder.

4. **Image digest pinning**
   - Soak must prove immutable digest/SHA tag, not mutable `latest`.

5. **Isolated rehearsal secrets/overrides**
   - Bad Soniox/ElevenLabs drills must not mutate shared production `brivva/env`.
   - Add separate rehearsal secret, ECS task override path, or provider-failure test flag.

6. **Platform visible-live checks**
   - At minimum manual dashboard checklist.
   - Later automate YouTube Live API / Grip API if available.

7. **JSON summary**
   - Existing tracker says JSON summaries are future work.
   - For RS-007, manual `verdict.md` is acceptable, but JSON is better for repeatability.

## 7. Verdict Template

Write `verdict.md` after every run:

```markdown
# AWS Soak Verdict

Run: YYYYMMDD-HHMM-profile
Commit: <sha>
Image digest: <digest>
Task definition: <arn>
Instance: <id> / <type> / <az>
Duration: <minutes>
Platforms: <list>
Languages: <list>

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
- Notes: ...

## First Bad Timestamp If Failed

UTC timestamp:
Log line:
Likely class: AWS / FFmpeg / platform / Soniox / ElevenLabs / billing / operator

## Decision

- RS-007 status: open/watch/patched
- Next patch:
```

## 8. RS-007 Closure Criteria

Move RS-007 from `open` to `patched` only after:

1. dry-run capability gate passes on AWS;
2. single real platform smoke passes on AWS;
3. exact 30-60 min launch-profile soak passes on AWS;
4. provider/platform failure drills pass or have documented launch-safe manual fallback;
5. billing verification shows unbillable windows excluded;
6. run artifacts are saved under `tmp/aws-soak-runs/...` or durable docs/log storage;
7. `docs/rust-stress-test/tracker.md` is updated with commit/run ID and remaining watch items.

If exact-profile soak passes but failure drills are incomplete, set RS-007 to `watch`, not `patched`.

## 9. Immediate Next Actions

1. Decide proof mode:
   - private `brivva-gpu` fake-sink: lowest blast radius, GPU/image only;
   - private `brivva-gpu` real-platform: first add NAT/external egress + operator/browser access;
   - primary `brivva` EC2_GPU cutover: real path, higher blast radius, rollback ready.
2. Verify helper scripts from `infra/README.md`; restore or replace missing ones with region-explicit AWS CLI commands.
3. Add/confirm AWS runtime self-check logs for FFmpeg/NVENC/fonts; do not depend on ECS Exec unless infra enables it.
4. Add isolated rehearsal secret/task-override path for bad provider drills.
5. Build/push GPU image and record immutable digest/SHA tag.
6. Scale `brivva-gpu` to one host/task only after network mode is ready.
7. Run dry-run capability probe.
8. Run fake/local sink smoke.
9. Run 10-15 min YouTube smoke only from real-platform-capable network mode.
10. Run Grip smoke with fresh one-shot key if Grip is in launch.
11. Run 30-60 min exact launch-profile soak.
12. Run failure drills.
13. Update tracker.
