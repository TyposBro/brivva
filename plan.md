# Brivva Tech — SaaS Migration Plan

**Date:** April 16, 2026
**Status:** Deal confirmed. Pivoting desktop app → cloud SaaS.

---

## Phase 1: Fix Demo Failures (Apr 17-20)

| # | Task | Details | Effort |
|---|---|---|---|
| 1 | **Per-language video delay** | Replace global `broadcast_delay` with per-language `delay_ms` in stream config. Source lang = 0ms. Each target lang configurable (ja=1000, zh=3000). | 2 days |
| 2 | **Audio mixing** | Each target stream: host audio at 20% volume (passthrough) + TTS at 100%. Mix in audio drain before FFmpeg. Source stream: host audio at 100%, no TTS. | 1 day |
| 3 | **Voice cloning bug** | Indian accent in English. Reproduce: check voice_id resolution, language param passed to TTS, clone vs default voice selection logic. | 1 day |
| 4 | **RTMPS support** | FFmpeg supports `rtmps://` natively. Update RTMP URL validation to accept `rtmps://`. Test with Grip Live. | 0.5 day |

**Do NOT build during Phase 1:**
- SaaS migration
- Dashboard
- Billing
- New features

---

## Phase 2: SaaS Architecture (Apr 21 - May 4)

### Week 1: Pipeline Containerization (Apr 21-27)

| # | Task | Details |
|---|---|---|
| 1 | **Strip Tauri** | Remove `src-tauri/`, Tauri dependencies. Keep `server-rs/` as standalone Axum server + `frontend/` as React SPA. |
| 2 | **Dockerfile** | Multi-stage: build Rust binary → runtime with FFmpeg. Alpine or Debian slim. |
| 3 | **Terraform infra** | `infra/` directory. Modules: VPC, ECR, ECS cluster, Fargate task definition (1 vCPU, 2GB RAM), security groups (inbound WS, outbound all), IAM roles (task execution + task role), CloudWatch log group. State in S3 backend. |
| 4 | **Terraform deploy** | `terraform apply` → ECR repo + ECS cluster ready. Push Docker image. Test: RunTask → connect WS → 30min session → verify RTMP output. |
| 5 | **Usage reporting** | Container POST output_minutes to billing endpoint on session end. |

### Week 2: Cloudflare Layer (Apr 28 - May 4)

| # | Task | Details |
|---|---|---|
| 1 | **D1 schema** | `users`, `sessions`, `usage_records`, `voice_configs`, `invoices` tables. |
| 2 | **Google OAuth** | CF Worker: OAuth flow → JWT. Session tokens. User creation on first login. |
| 3 | **Session API** | `POST /api/sessions` → RunTask on Fargate → return WS URL. `DELETE /api/sessions/:id` → StopTask. |
| 4 | **Dashboard** | CF Pages: React SPA. Login → session list → create session (language config, RTMP URLs) → live session view. |
| 5 | **R2 integration** | Voice samples upload to R2. Container downloads from R2 on session start. |

---

## Phase 3: Production Polish (May 5-18)

| # | Task | Details |
|---|---|---|
| 1 | **Billing metering** | Track output minutes per session per client. Monthly aggregation. Invoice generation. |
| 2 | **Client onboarding** | Sign up → configure languages → upload voice sample → set RTMP destinations → first stream. |
| 3 | **Voice library** | Browse ElevenLabs voices from dashboard. Preview. Select per language. |
| 4 | **Cold start optimization** | Fargate cold start ~30s. Pre-warm strategy: keep 1 container warm during business hours. |
| 5 | **RTMPS + multi-platform** | Multiple RTMP destinations per language (YouTube + Grip Live + Coupang simultaneously). |
| 6 | **Monitoring** | CloudWatch alarms: container crashes, high latency, API errors. Forward to dashboard. |

---

## Phase 4: Scale (May 18+)

- Tier 4 dubbing from dashboard (post-session, v3 enhanced model)
- Auto-language detection (skip manual config)
- Per-host voice profiles (save voice settings across sessions)
- Analytics dashboard (minutes used, quality scores, cost breakdown)
- Multi-region Fargate (ap-northeast-1 for Korea, ap-southeast-1 for SEA)

---

## Architecture Decisions

| Decision | Choice | Why |
|---|---|---|
| Pipeline hosting | AWS Fargate | Long-running sessions (40min+), FFmpeg, WebSockets. Can't run on CF Workers. |
| Auth/billing/dashboard | Cloudflare (Workers + Pages + D1 + R2) | Aziz knows CF stack (Spiko uses it). Free/cheap. Fast. |
| Container per session | Yes | Isolation. No noisy neighbors. Clean shutdown. Simple scaling. |
| Scale to zero | Yes | No cost when no sessions active. Fargate charges per second. |
| API keys | Server-side only | Never exposed to client. Container has env vars from ECS task definition. |
| Frontend | React SPA on CF Pages | Existing frontend code, just strip Tauri-specific parts. |
| Database | CF D1 | Good enough for billing + session management. Aziz has 107 tables in Spiko on D1. |

---

## Migration Checklist

**From desktop app:**
- [ ] Strip Tauri shell (keep server-rs + frontend)
- [ ] Standalone Axum server (no Tauri invoke, direct HTTP/WS)
- [ ] Dockerfile builds and runs locally
- [ ] FFmpeg sidecar works in container
- [ ] WebSocket connection from browser to container works
- [ ] RTMP output works from container
- [ ] 30min endurance test passes in container

**Terraform (infra/):**
- [ ] S3 backend for state
- [ ] VPC + subnets (public, for Fargate tasks with public IP)
- [ ] ECR repository
- [ ] ECS cluster + Fargate task definition
- [ ] IAM roles (task execution + task role with R2/secrets access)
- [ ] Security groups (inbound WS port, outbound all)
- [ ] CloudWatch log group
- [ ] `terraform plan` clean, `terraform apply` succeeds
- [ ] CF Workers: Google OAuth working
- [ ] CF D1: schema deployed
- [ ] CF Pages: dashboard deploys
- [ ] Session API: create → Fargate task → WS URL
- [ ] Session API: stop → kill task → report usage
- [ ] R2: voice sample upload/download working

---

## Key Files

| File | Purpose |
|---|---|
| `context.md` | Full technical context for LLM collaborators |
| `plan.md` | This file — migration roadmap |
| `tier-4.md` | ElevenLabs Dubbing API integration docs |
| `claude.md` | Architecture rules (4-layer, dependency direction) |
| `todo.md` | Granular task tracking |
| `infra/` | Terraform modules (VPC, ECR, ECS, Fargate, IAM, CloudWatch) |
