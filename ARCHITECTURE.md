# Brivva Architecture

## Current (Phase 1)

Everything on Fargate.

```
┌─────────────────────┐       ┌─────────────────────────────────┐
│ Cloudflare Pages    │       │ Fargate task (us-east-1, x86_64)│
│ brivva.pages.dev    │──────▶│ brivva.spiko.uz (via cloudflared)│
│ React SPA (Vite)    │  WSS  │                                 │
└─────────────────────┘  HTTPS│  • server-rs (Rust)             │
                              │    - WS ingest (host audio/video)│
                              │    - Soniox STT client          │
                              │    - ElevenLabs TTS client      │
                              │    - ffmpeg RTMP muxer (per lang)│
                              │    - SQLite (ephemeral /tmp)    │
                              │    - YouTube OAuth + broadcasts │
                              │    - Platform credentials CRUD  │
                              │  • cloudflared-init (alpine)    │
                              │  • cloudflared (distroless)     │
                              └─────────────────────────────────┘
                                           │  RTMP out
                                           ▼
                              ┌─────────────────────────────────┐
                              │ YouTube Live / Twitch / IG / ...│
                              └─────────────────────────────────┘
```

**State today:** SQLite at `/tmp/brivva.db` — **ephemeral, lost on task restart.**

## Target (Phase 3)

Split: **Workers + D1 for state**, **Fargate for media path only**.

```
┌─────────────────────┐        ┌──────────────────────────────┐
│ Cloudflare Pages    │  HTTPS │ Cloudflare Workers           │
│ brivva.pages.dev    │───────▶│ brivva-api.workers.dev       │
│ React SPA           │        │  • OAuth (Google YouTube)    │
│                     │        │  • users / voices / creds    │
│                     │        │  • sessions / streams CRUD   │
│                     │        │  • D1 (SQLite at edge)       │
│                     │        │  • Issue JWT on login        │
│                     │        └──────────────────────────────┘
│                     │                     │ JWT
│                     │                     ▼
│                     │ WSS+JWT ┌──────────────────────────────┐
│                     │────────▶│ Fargate (us-east-1, x86_64)  │
│                     │         │ brivva.spiko.uz              │
└─────────────────────┘         │  • Verify JWT on WS connect  │
                                │  • WS ingest                 │
                                │  • Soniox STT                │
                                │  • ElevenLabs TTS            │
                                │  • ffmpeg RTMP muxer         │
                                │  • ZERO persistent state     │
                                └──────────────────────────────┘
                                          │ RTMP
                                          ▼
                                 YouTube / Twitch / IG / ...
```

## Why this split

| Concern | All-Fargate (now) | Workers + Fargate (target) |
|---|---|---|
| OAuth tokens, voice IDs, RTMP creds | SQLite+EFS ($$) or ephemeral (lossy) | D1 — free, 5M reads + 100k writes/day, edge replicated |
| Cold start for `GET /api/sessions` | 60-90s Fargate cold | <10ms Worker cold |
| Global latency for API | 200ms+ transatlantic | 20ms edge |
| Cost at low traffic | $15/mo Fargate always on | ~$0 Workers + $15 Fargate |
| Horizontal scale per room | Fargate task pinning (hard) | Workers handle routing naturally |
| SQLite persistence problem | Needs EFS mount (slow, $$) | D1 solves it natively |

## Critical gotcha — WS belongs on Fargate

**Workers support WebSockets via Durable Objects, but:**

- Max **2 hours** idle before hibernation
- CPU budget per message (small)
- Not designed for multi-minute continuous audio streams

**Therefore:** long-lived media WebSocket (host browser → server, 10+ minutes of audio at 44.1 kHz) **must stay on Fargate**. Workers handle only the control plane.

**Do NOT** try to proxy the media WS through Workers — it adds a hop, the hop has CPU limits, and it gains you nothing. Frontend opens **two** connections:

1. `HTTPS fetch` to `brivva-api.workers.dev` — login, session create, voice clone enroll
2. `WSS` direct to `brivva.spiko.uz/ws?token=<jwt>` — audio/video stream

Control plane on Workers, hot path on Fargate. That's the rule.

## Auth bridge (Phase 3)

1. User logs in via Workers → Google OAuth
2. Workers issues a short-lived JWT (15min), signed with `HS256` + shared secret
3. Frontend opens WS: `wss://brivva.spiko.uz/ws?token=<jwt>`
4. Fargate verifies the JWT signature with the same shared secret (stored in Secrets Manager)
5. On accept: Fargate looks up voice_clone_id + session_id from JWT claims (no DB call)

Shared secret rotation: generate new secret in both Workers secrets and AWS Secrets Manager, overlap validity for 15min, kill old.

## Phased migration plan

### Phase 1 — ship what we have (today)

- ✅ Terraform infra (Fargate x86_64, cloudflared tunnel, ECR)
- ✅ `server-rs` deployed, tunnel reachable at `https://brivva.spiko.uz/`
- ⬜ Wire Soniox v4 STT client (replace legacy stt-wrapper call site)
- ⬜ Deploy frontend to Cloudflare Pages (`brivva.pages.dev`) pointing at Fargate backend
- ⬜ End-to-end test: browser → WS → STT → translate → TTS → RTMP

Exit: demo works. All state still on Fargate SQLite (ephemeral).

### Phase 2 — stand up Workers + D1

- Create Workers project `brivva-api`
- Create D1 database `brivva`, run schema migration (port of `server-rs/src/db.rs` DDL to SQLite syntax — it already IS SQLite so 1:1)
- Port CRUD endpoints to Workers:
  - `GET/POST /api/user` (users table)
  - `GET/POST /api/voices` (voices table; proxies ElevenLabs clone enroll)
  - `GET/POST/DELETE /api/credentials` (platform_credentials)
  - `GET/POST /api/sessions` (sessions + streams)
- Frontend gets a second base URL `VITE_API_URL=https://brivva-api.<zone>.workers.dev`
- Fargate keeps serving API endpoints in parallel during migration (double-write period to avoid data loss)

Exit: API calls can hit either Fargate or Workers; both reads + writes work.

### Phase 3 — move OAuth + auth bridge, strip Fargate backend

- Move Google OAuth callback to Workers (`/auth/youtube/callback`) — update redirect URI in Google Console
- Issue JWT on successful login, frontend stores it
- Fargate gains `verify_jwt(token)` gate on WS upgrade
- Strip `server-rs` of: `db.rs`, `routes.rs` (except WS handler), `youtube.rs`, session/stream CRUD
- Fargate becomes **pure media**: STT → translate → TTS → ffmpeg RTMP

Exit: Fargate is stateless, D1 is canonical store. Deploys of Fargate no longer risk data loss. Ready to horizontally scale Fargate per room.

### Phase 4 (nice-to-have, later)

- Fargate service autoscaling (target ActiveConnections / room count)
- One task per host (room routing via Cloudflare Worker or Service Connect)
- Burn-in subtitle drawtext wiring
- Fargate Spot capacity provider (~70% cost cut vs on-demand)
- Move to EC2 Spot if cost > ops matter more

## State ownership matrix

| Data | Lifetime | Where (now) | Where (Phase 3) |
|---|---|---|---|
| YouTube OAuth tokens | Weeks-months | Fargate SQLite (ephemeral) | D1 |
| ElevenLabs voice clone IDs | Forever | Fargate SQLite | D1 |
| Saved RTMP credentials | Forever | Fargate SQLite | D1 |
| Session metadata | Session | Fargate SQLite + in-memory | D1 + Fargate memory |
| Active stream state | Session | Fargate memory | Fargate memory only |
| Live audio/video frames | Seconds | Fargate memory | Fargate memory |
| ffmpeg process handles | Session | Fargate memory | Fargate memory |

## Non-goals

- Multi-region. US-East-1 only (Soniox + ElevenLabs live there).
- GPU. CPU transcode fits within 2 vCPU budget.
- On-prem. Managed cloud only.
- Self-hosted STT/TTS/translate. Managed APIs.

## Incident Response (May 10 Live Launch)

**Reality:** first live stream is `May 10` with real Korean hosts + real clients. Aziz alone responds. Something will break. Pre-decide the playbook now, not at 2am.

### Rollback
- Every deploy tagged `v-YYYYMMDD-HHMM` (git tag + ECR image tag matching)
- One command restores previous working version in `<2min`
  - Fargate: `aws ecs update-service --force-new-deployment --task-definition <prev-rev>`
  - Workers: `wrangler rollback` or re-deploy prior commit
  - Frontend (Pages): redeploy prior commit via dashboard or CI
- Rehearse once before `May 10`

### Kill-Switches (env vars, no redeploy needed)

| Var | Effect |
|---|---|
| `BRIVVA_DISABLE_TIER4=1` | Skip post-processed dubbing if ElevenLabs Dubbing API breaks |
| `BRIVVA_FALLBACK_TO_DEFAULT_VOICE=1` | Skip voice cloning if it produces garbage (Indian accent bug) |
| `BRIVVA_DISABLE_QWEN3=1` | Force ElevenLabs only if DashScope goes down |
| `BRIVVA_FORCE_RTMP_NOT_RTMPS=1` | Drop to unsecured RTMP if TLS handshake fails with a platform |

### Observability Under Stress
- `wrangler tail` — Workers logs, pre-opened terminal
- `aws logs tail /ecs/brivva-tech --follow` — Fargate logs, pre-opened terminal
- CloudWatch dashboard bookmarked (CPU, memory, task count, error rate)
- Within 30s of an alert, must identify the failing layer: frontend / auth / pipeline / third-party

### Plan B — Graceful Degradation
- **Desktop app always works.** If SaaS breaks during a live show, Simon demos off Aziz's laptop via Tauri build = show continues.
- Voice clone broken → default voice per language (already in voice library)
- Real-time pipeline broken → Tier 4 post-processed delivered after the show (better than no output)
- Soniox WS drops → reconnect 5 attempts, 1s delay (already built)
- ElevenLabs WS slow → REST fallback (already built)
- ElevenLabs API outage → fall back to default voice cache on disk

### Third-Party Failure Matrix

| Service | Failure | Fallback |
|---|---|---|
| Soniox | WS drop | Auto-reconnect (built) |
| ElevenLabs WS | Slow | REST fallback (built) |
| ElevenLabs API | Total outage | Kill-switch → default voice cache |
| Grip RTMPS | TLS handshake fails | `BRIVVA_FORCE_RTMP_NOT_RTMPS=1` if platform allows |
| Fargate | `~30s` cold start | Pre-warm 1 container during business hours |
| CF Workers | Auth broken | Desktop app works without auth layer |

### Human Factors
- Aziz = sole responder. No rotation.
- Runbook for top 3 likely failures (TTS timeout, RTMP disconnect, browser WS drop) — `docs/runbook.md` (to write)
- Simon + MJ need a script to tell clients during incident ("technical issue, back shortly — ETA Xmin")

### Recovery Validation
- `./scripts/smoke-test.sh <session_id>` — creates session, connects WS, streams 30s test audio, validates RTMP output, reports pass/fail
- Run after every rollback. Do not assume fix until smoke test green.

## Grip Live — Known Quirks

Grip is the primary streaming target for Korean live commerce. Less documented than YouTube/TikTok.

- **RTMPS URL:** must include explicit port `:443`, not default. Format: `rtmps://live.grip.fans:443/live/<STREAM_KEY>`
- **Stream key:** longer than YouTube/TikTok, contains session auth token. Fetch fresh per stream (no static key).
- **Regional endpoints:** some drop after `~30min` idle. Need reconnect logic on FFmpeg side.
- **Stream health API:** undocumented. Do not poll it. Poll Grip's dashboard URL or rely on FFmpeg exit codes.
- **Credentials:** shared by Simon already, stored in `platform_credentials` table.
