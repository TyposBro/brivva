# Brivva Tech — Cloud + API Cost Estimate

**Date:** 2026-04-20
**Prepared by:** Aziz (tech lead)
**Purpose:** Reimbursable infrastructure + API costs for Brivva Tech
**Scope:** Everything required to run the SaaS end-to-end — AWS,
Cloudflare, Soniox (STT + translation), ElevenLabs (voice cloning +
TTS), and Aziz's development tools.

---

## Executive Summary

| Stage | Monthly Ask (reimbursable) | Revenue | Margin |
|---|---|---|---|
| Pre-launch (now → May 10) | **~$360/mo** fixed + per-show variable | — (testing only) | — |
| Launch month (May, first clients) | **~$520/mo** | ~$2,700/mo (3 clients × 3 langs) | ~81% |
| Steady state (10 clients × 3 langs) | **~$920/mo** | ~$9,000/mo | **~90%** |
| Peak (10 clients × 5 langs, SEA live) | **~$1,760/mo** | ~$22,500/mo | **~92%** |
| + Aziz dev tool (ChatGPT Pro $200/mo) | **+$200/mo** | — | — |

**Bottom line:** at steady state, total reimbursable cloud + API + dev
tools ≤ **$1,120/mo** against **$9,000/mo** revenue = **12% of revenue,
88% margin.** Matches the `vision.md` §Unit Economics 90% projection —
no surprises.

---

## Unit Costs (what drives everything else)

| Item | Rate | Notes |
|---|---|---|
| Soniox real-time STT + translation | **~$0.11 per source-audio hour** | All features included: transcription, diarization, 60+ langs. |
| ElevenLabs Flash v2.5 TTS (Direct API) | **$0.06 per 1,000 chars** ≈ $0.09/output-min | Per-character, no subscription required. |
| AWS Fargate (8 vCPU / 16 GB task) | **$0.395 per hour** | $0.04048/vCPU-hr + $0.004445/GB-hr, us-east-1. |
| AWS data transfer out (RTMP push) | **$0.09 / GB** | 1080p @ 4 Mbps ≈ 1.8 GB/hr per destination. |
| Cloudflare Workers + D1 + Pages | **$0** | Free tier until ~5M req/mo; current volume is near zero. |
| ChatGPT Pro (Aziz dev tool) | **$200 / mo** | 20× Plus limits, Codex, o3 pro reasoning, deep research. |

### Why Fargate 8 vCPU (bumped from 2 vCPU on 2026-04-20)

The "ultimate test" shape promised to Simon + Yuni
(`En → Ko+Zh+Ja + passthrough`, broadcasting to Grip + TikTok +
YouTube simultaneously) requires:

| Work | CPU cost |
|---|---|
| 1080p30 video transcode × 4 targets (3 translations + 1 passthrough) | ~3.4 cores |
| drawtext subtitle overlay × 3 translations | ~0.25 cores |
| Audio mixing + STT/TTS orchestration | ~0.5 cores |
| OS + tokio runtime overhead | ~0.3 cores |
| **Total peak usage** | **~4.5 cores** |

The previous 2 vCPU ceiling was too tight. The new 8 vCPU / 16 GB
sizing covers the ultimate test with **3.5 cores of headroom**, which
also accommodates 5-lang SEA streams at steady state without a second
Terraform bump.

---

## Scenario A — Single 1-hour show (May 10 "ultimate test")

```
Host speaks English.
Outputs translated: Korean (to Grip), Chinese (to TikTok),
                   Japanese (to YouTube), + passthrough on one platform.
Duration: 1 hour.
```

| Line | Cost |
|---|---|
| Soniox (1 source-hour of STT + translation) | $0.11 |
| ElevenLabs (3 targets × 60 min × ~1,500 chars/min × $0.06/1000) | $16.20 |
| AWS Fargate 8 vCPU marginal (1 hour of an always-on task) | $0.40 |
| AWS data transfer out (3 × 4 Mbps × 1 hour = 5.4 GB × $0.09) | $0.49 |
| **Variable cost, single 1-hour show** | **~$17.20** |

---

## Scenario B — Monthly fixed infrastructure (always-on, independent of show count)

| Line | Monthly |
|---|---|
| AWS Fargate 8 vCPU / 16 GB × 24×7 (1 task) | $284 |
| AWS Application Load Balancer | $23 |
| AWS NAT Gateway | $32 |
| AWS CloudWatch (logs + metrics + alarms) | $15 |
| AWS ECR image storage + Route 53 + misc | ~$6 |
| Cloudflare Workers (backend API) | $0 |
| Cloudflare D1 (database; 94 KB today, 5 GB free tier) | $0 |
| Cloudflare Pages (frontend hosting) | $0 |
| **Fixed infra** | **~$360 / mo** |

*Fargate always-on is the conservative / simplest model. Phase 2
auto-scaling with a small warm pool drops this to ~$80/mo by
scaling-to-zero outside scheduled show hours. Not a May 10 priority,
but worth budgeting against.*

---

## Scenario C — Steady state (10 B2B clients × 200 source-min × 3 target langs)

Matches `vision.md` §Unit Economics revenue projection.

| Line | Monthly |
|---|---|
| Fixed infra (Scenario B) | $360 |
| Soniox: 2,000 source-min × $0.002 | $4 |
| ElevenLabs: 6,000 output-min × $0.09 | $540 |
| AWS data transfer out: 180 GB × $0.09 | $16 |
| **Total reimbursable cloud + API** | **~$920 / mo** |
| **Revenue:** 6,000 output-min × $1.50/min | $9,000 / mo |
| **Profit** | **~$8,080 / mo** |
| **Margin** | **~90%** ✅ |

---

## Scenario D — Peak (10 clients × 300 source-min × 5 target langs, SEA live)

| Line | Monthly |
|---|---|
| Fixed infra (Scenario B) | $360 |
| Soniox: 3,000 source-min × $0.002 | $6 |
| ElevenLabs: 15,000 output-min × $0.09 | $1,350 |
| AWS data transfer out: 450 GB × $0.09 | $41 |
| **Total reimbursable cloud + API** | **~$1,760 / mo** |
| **Revenue:** 15,000 × $1.50 | $22,500 / mo |
| **Margin** | **~92%** ✅ |

---

## Aziz Development Tools — ChatGPT Pro $200

**$200 / month** — 20× Plus limits, full Codex access, o3 pro
reasoning tier, unlimited deep research, parallel project workflows.

Covers:

- Daily dev assistance (code generation, debugging, architecture reviews)
- Large-context codebase reasoning (Brivva spans server-rs Rust +
  Workers TypeScript + React frontend + Terraform + CI/CD)
- Architecture spike workflows (Phase 2 WebRTC, horizontal scaling,
  multi-speaker, B2B admin panel, SEA expansion)
- Codex CLI for local agentic task execution (~20× faster than
  browser-based workflows for multi-file edits)

**Annual: $2,400.** Rounding error against steady-state monthly
revenue.

---

## Summary Table (put this in the approval email)

| Stage | Fixed infra | Variable API/transfer | Dev tool | **Total ask** |
|---|---|---|---|---|
| Now → May 10 | $360 | ~$17/show hour | $200 | **$560 + per-show** |
| Launch month (~3 clients × 3 langs) | $360 | ~$162 | $200 | **~$720** |
| Steady state (10 clients × 3 langs) | $360 | ~$560 | $200 | **~$1,120** |
| Peak (10 clients × 5 langs) | $360 | ~$1,400 | $200 | **~$1,960** |

**At steady state:** total spend ≤ 12% of revenue, margin ≥ 88%.

---

## Watch-outs (transparency for Simon)

### 1. Fargate always-on is the single largest fixed line ($284/mo)

Today's 1-task architecture burns ~$284/mo regardless of whether any
shows are running. Phase 2 horizontal scaling (design doc due
post-May-10) drops this to ~$80/mo by:

- Scaling task count to zero when no active stream
- Keeping a 1-task warm pool ($80/mo) so cold-start latency stays
  acceptable
- Auto-scaling out on `ActiveStreamCount` metric

Saves ~$200/mo once implemented. Phase 2 engineering work, ~3 weeks.

### 2. ElevenLabs is the dominant variable cost

At peak scenario (5 langs × 10 clients), ElevenLabs alone is
**$1,350/mo** — 77% of variable spend. Once monthly spend crosses
~$1,000, we should open an **ElevenLabs Enterprise conversation** for
negotiated per-character pricing. Typical enterprise discounts are
30–50% below PAYG, which would drop the peak scenario by $400-600/mo.

### 3. Soniox is effectively free at Brivva's scale

Even at peak (3,000 source-min/mo), Soniox spend is **$6/mo**. Not a
line item worth optimizing. Included for transparency.

### 4. Cloudflare is free until late-stage growth

Workers + D1 + Pages are on the free tier. Estimated tier-jump at
**~5 million API requests per month** — well beyond 10-client scale.
Budget $5–20/mo once we cross, Phase 3+ concern.

### 5. Data transfer scales with target-platform count

Each new RTMP target adds ~1.8 GB/hr at 1080p30. At 5 langs × 300
min × 10 clients = 450 GB/mo × $0.09 = $41/mo. Not large. Could
future-optimize with CloudFront egress pricing if it materially grows.

---

## Pricing Sources (verified 2026-04-20)

- **Soniox** — [Pricing page](https://soniox.com/pricing). Token-based;
  effective rate ~$0.11/hour of real-time STT + translation, all
  features included.
- **ElevenLabs** — [Pricing page](https://elevenlabs.io/pricing).
  Flash v2.5 multilingual TTS at $0.06 per 1,000 characters direct
  API. Business-tier subscription ($1,320/mo for 11M chars) is worse
  than direct API for Brivva's volume — stick with PAYG.
- **AWS Fargate** — [Pricing page](https://aws.amazon.com/fargate/pricing/).
  $0.04048/vCPU-hr + $0.004445/GB-hr, us-east-1.
- **AWS data transfer** — [EC2 pricing](https://aws.amazon.com/ec2/pricing/on-demand/).
  $0.09/GB for first 10 TB/mo outbound.
- **Cloudflare Workers** — [Pricing](https://developers.cloudflare.com/workers/platform/pricing/).
  Free tier includes 100K requests/day; paid tier $5/mo base.
- **Cloudflare D1** — [Pricing](https://developers.cloudflare.com/d1/platform/pricing/).
  Free tier includes 5 GB storage + 25B reads/day.
- **ChatGPT Pro** — [Pricing page](https://chatgpt.com/pricing/).
  Pro $200/mo tier: 20× Plus limits, full Codex, o3 pro reasoning,
  unlimited deep research.

---

## Appendix — Fargate sizing math

```
Target: May 10 "ultimate test"
  English source → Ko + Zh + Ja translations + passthrough
  broadcast to Grip + TikTok + YouTube simultaneously

Per-target CPU (1080p30 @ 4 Mbps):
  libx264 transcode:         ~0.80 core
  drawtext subtitle overlay: ~0.08 core  (translation targets only)
  AAC audio encode:          ~0.05 core

Per-target total:
  Translation target:  0.93 core
  Passthrough target:  0.85 core (no drawtext)

Ultimate test total:
  3 translations × 0.93 =  2.79 cores
  1 passthrough × 0.85 =   0.85 cores
  STT/translate/TTS orch:  0.50 cores
  OS + tokio runtime:      0.30 cores
                           ─────
                           4.44 cores

Current task (pre-bump):  2 vCPU  ❌ over budget
New task (post-bump):     8 vCPU  ✅ 3.56 cores headroom

Steady-state 5-lang SEA:
  5 translations × 0.93 =  4.65 cores
  1 passthrough × 0.85 =   0.85 cores
  orchestration + OS:      0.80 cores
                           ─────
                           6.30 cores  ✅ fits in 8 vCPU
```

---

*End of cost estimate.*
