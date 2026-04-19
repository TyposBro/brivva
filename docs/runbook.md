# Runbook — May 10 Launch and Beyond

Status: **active, owner = Aziz (sole responder)**.

When something breaks mid-stream and Simon is on the phone, this file
answers "what do I do right now". Everything here maps to the Incident
Response section in `ARCHITECTURE.md`.

---

## Top 3 Likely Failures

### 1. Translated TTS silent — host audio fine, translated stream has no voice

Symptom: host speaks, subtitles appear, but translated output is silence
or garbled. Usually ElevenLabs WS stall, voice clone id mismatch, or
Qwen3 auth issue.

Triage (30 seconds):
1. Open Fargate logs: `aws logs tail /ecs/brivva --follow --since 5m`.
2. Grep for `[TTS]` — is the session even reaching TTS, or stuck at STT?
3. If `TTS speak request failed` or `DashScope error` → see fix.

Fix:
- `BRIVVA_DISABLE_QWEN3=1` → force ElevenLabs for all languages. Update
  secret in Secrets Manager, restart task. **Kills DashScope path, keeps
  stream alive.**
- If ElevenLabs itself is down: `BRIVVA_FALLBACK_TO_DEFAULT_VOICE=1`
  forces the built-in voice library (no clone). Lower quality, always
  works.

Recovery validation:
- Listen to RTMP output: `ffplay rtmps://...:443/live/<STREAM_KEY>`.
- Run `./scripts/smoke-test.sh <session_id>` (once that script exists).

### 2. RTMP disconnect mid-show — FFmpeg dies, stream drops

Symptom: viewers see "stream offline", Grip dashboard shows red. FFmpeg
log has `Connection reset` or `Broken pipe`.

Triage:
1. Check FFmpeg exit code in Fargate logs.
2. If Grip-specific: known regional endpoints drop after ~30min idle —
   reconnect should auto-fire, give it 10s.
3. If still dead after 30s: platform outage or credential rotation.

Fix:
- Restart session from dashboard (frontend: "Stop" then "Start"). This
  triggers a new FFmpeg spawn with fresh stream key.
- If Grip TLS handshake fails: `BRIVVA_FORCE_RTMP_NOT_RTMPS=1` →
  unsecured RTMP. Only if platform allows it (check with Simon).

Recovery validation:
- Stream appears in platform dashboard with live viewers.
- ffprobe reports audio + video tracks on the RTMP output.

### 3. WebSocket drop — browser ↔ Fargate connection lost

Symptom: dashboard shows "disconnected", session state frozen, no audio
frames reaching server.

Triage:
1. Browser dev console → Network → WS frame — server closed, or client?
2. Check Fargate logs for `WS upgrade rejected` (JWT expiry) or
   `connection reset by peer` (cloudflared tunnel flap).
3. If cloudflared → DNS propagation or tunnel token issue.

Fix:
- Refresh browser tab — re-fetches JWT, reconnects WS. **Fixes 80% of
  cases.** Session state restores from Workers / D1.
- If JWT expired mid-session: check `JWT_SECRET` didn't rotate in
  Secrets Manager. If so, re-login forces new JWT.
- If cloudflared tunnel flapping: `aws ecs update-service --force-new-deployment`
  on the cluster → restarts `cloudflared-init` sidecar.

Recovery validation:
- Dashboard reconnects, session state matches pre-drop.
- Host audio flows again, TTS resumes within 5s.

---

## Kill Switches (env vars, no redeploy)

Update Secrets Manager `brivva/env` → force task restart:

| Var | When to use |
|---|---|
| `BRIVVA_DISABLE_TIER4=1` | Dubbing (ElevenLabs Dubbing API) broken, skip post-processing |
| `BRIVVA_FALLBACK_TO_DEFAULT_VOICE=1` | Voice clone producing garbage (Indian accent), force default voices |
| `BRIVVA_DISABLE_QWEN3=1` | DashScope down, force ElevenLabs only |
| `BRIVVA_FORCE_RTMP_NOT_RTMPS=1` | TLS handshake fails with a platform, drop to unsecured |

Apply:
```bash
aws secretsmanager update-secret --secret-id brivva/env --secret-string '{...}'
aws ecs update-service --cluster brivva --service brivva --force-new-deployment
```

---

## Rollback — restore last-known-good in <2 min

Every deploy is tagged `v-YYYYMMDD-HHMM` on both git and ECR.

```bash
# find last green tag
git tag --sort=-committerdate | head -5

# redeploy the matching ECS task definition
aws ecs update-service \
  --cluster brivva \
  --service brivva \
  --task-definition brivva:<PREV_REVISION>

# workers rollback
cd workers && wrangler rollback

# pages rollback — use dashboard or redeploy prior commit
```

Rehearse this **before May 10**. Practice makes 2am fast.

---

## Logs — Pre-Opened Terminals

Have these tabs open during any live show:

```bash
# Fargate (server-rs)
aws logs tail /ecs/brivva --follow

# Workers
cd workers && wrangler tail

# CloudWatch dashboard (bookmarked)
# https://console.aws.amazon.com/cloudwatch/home?region=us-east-1#dashboards:name=brivva
```

Within 30s of an alert, must know which layer is broken:
frontend / auth (Workers) / pipeline (Fargate) / third-party.

---

## Plan B — Desktop App Fallback

If the SaaS path breaks during a live show and rollback fails, Simon
switches to Tauri desktop build. Same Rust pipeline, no CF/Fargate
dependency.

```bash
# Aziz's laptop — keep a signed build ready
cd /Users/typosbro/Documents/private/brivva
cargo tauri build
# App is at src-tauri/target/release/bundle/macos/brivva.app
```

Simon runs the show off Aziz's laptop. Show continues. Fix SaaS
post-show.

---

## Simon / MJ Client Communication Script

When the stream breaks mid-show, Simon tells the client:

> "We hit a technical issue on the translation pipeline. Engineer is
> on it — back within 5 minutes. Recorded VOD with translation will be
> delivered regardless."

Don't explain internals. Give ETA + promise Tier 4 post-processed VOD
as compensation. Every session is recorded — VOD delivery works even
if live failed.

---

## What's NOT Here

- **Not an SLA.** We are pre-launch, no uptime commitments yet.
- **Not a postmortem template.** Write postmortems in `docs/postmortems/`
  after an incident.
- **Not a full operations manual.** This is the 2am-phone-call version.
  For architecture, read `ARCHITECTURE.md`.

---

## Pre-May-10 Checklist

- [ ] Rehearse rollback once (full cycle: break something, roll back, verify)
- [ ] Implement `scripts/smoke-test.sh` for post-rollback validation
- [ ] Verify all kill-switch env vars are actually read by server-rs
- [ ] Bookmark CloudWatch dashboard URL on phone browser
- [ ] Have Simon's + MJ's phone numbers saved, reachable at 2am KST
- [ ] Keep a signed Tauri build on laptop as Plan B
- [ ] Test `aws logs tail` command works from phone hotspot (if home WiFi dies)
