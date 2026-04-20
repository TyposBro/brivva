# Runbook — May 10 Launch and Beyond

Status: **active, owner = Aziz (sole responder)**.

When something breaks mid-stream and Simon is on the phone, this file
answers "what do I do right now". Everything here maps to the Incident
Response section in `ARCHITECTURE.md`.

---

## Top 3 Likely Failures

### 1. Translated TTS silent — host audio fine, translated stream has no voice

Symptom: host speaks, subtitles appear, but translated output is silence
or garbled. Usually ElevenLabs WS stall or voice clone id mismatch (the
April 2026 Indian-accent regression is the canonical example).

Triage (30 seconds):
1. Open Fargate logs: `aws logs tail /ecs/brivva --follow --since 5m`.
2. Grep for `[TTS]` — is the session even reaching TTS, or stuck at STT?
3. If the cloned voice is producing the wrong language/accent → kill-switch fix.
4. If ElevenLabs HTTP errors are the pattern → platform outage, skip to Plan B.

Fix:
- `BRIVVA_FALLBACK_TO_DEFAULT_VOICE=1` → force the target language's
  default voice from the ElevenLabs library, bypass cloning entirely.
  Update Secrets Manager, force-restart the ECS service. Lower voice
  fidelity but always works.
- If ElevenLabs itself is down: no kill-switch helps — ride out the
  outage, or switch to Plan B (desktop app fallback below).

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

### 2b. FFmpeg publishes to Grip/IVS but stream silently drops after 3 frames

Symptom: `ffmpeg rtmp stream started` in logs, then stderr goes quiet,
video drain exits with `write error` after a few seconds, audio drain
exits within ~1 second. Idle-detector SIGKILLs the child ~25s later.
Grip broadcast sits at `송출 대기중` forever. **No RTMP error in stderr.**
Repro-able with a plain ffmpeg CLI push to the same IVS URL.

Root cause: **ffmpeg's native RTMP implementation is incompatible with
AWS IVS ingest** (Grip runs on IVS). IVS accepts TCP+TLS+RTMP handshake
+ publish command, but never sends `onStatus NetStream.Publish.Start`
because the AMF metadata ffmpeg-native sends doesn't satisfy IVS's
client fingerprinting. The socket stays open for a few seconds then
IVS closes it server-side. ffmpeg doesn't surface this as an error.

**Only fix that works: ffmpeg built with `--enable-librtmp`** (the
library OBS uses). Verified 2026-04-20 — librtmp backend pushed 600
frames in 20s cleanly, broadcast flipped to `방송중`. Native backend
truncates at frame 3 every time, regardless of encoder preset, profile,
or stream key freshness.

Triage (30 seconds):
1. `ffmpeg -version | grep -o "enable-librtmp"` — must print the flag. If empty, that's the bug.
2. Debian bookworm's `apt-get install ffmpeg` does **not** include librtmp. macOS `brew install ffmpeg` core formula also doesn't.
3. Confirm with a ffmpeg CLI push using testsrc + sine to the same IVS URL — if it fails at 3 frames with librtmp missing, the fix is the same fix for server-rs.

Fix (local dev, macOS):
```
brew uninstall --ignore-dependencies ffmpeg
brew install homebrew-ffmpeg/ffmpeg/ffmpeg --with-openssl --with-rtmpdump --build-from-source
```

Fix (prod, Fargate): `server-rs/Dockerfile` must compile ffmpeg from
source with `--enable-librtmp --enable-openssl`. Runtime image needs
`librtmp1` + the source-built binary. Integration test should guard:
`ffmpeg -version | grep -q enable-librtmp` at container build time.

Non-fixes (tried, none work):
- Changing encoder preset (veryfast, ultrafast), profile (main, constrained baseline)
- Changing bitrate caps, keyframe interval, GOP size
- Rotating the stream key (keys are persistent per IVS channel anyway)
- Adjusting 예정일시 (scheduling window) — broadcast still rejects
- Explicit `-rtmp_tcurl`, `-rtmp_live live`, `-rtmp_flashver`

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

Update Secrets Manager `brivva/env` → force task restart. Both switches
are wired in `server-rs` and read once at session start per `AppConfig`.

| Var | When to use | Wired at |
|---|---|---|
| `BRIVVA_FALLBACK_TO_DEFAULT_VOICE=1` | Voice clone producing garbage (accent bugs), force default voices | `features/broadcast/data/pipeline/tts.rs` |
| `BRIVVA_FORCE_RTMP_NOT_RTMPS=1` | TLS handshake fails with a platform, drop to unsecured RTMP | `features/broadcast/data/session_ws.rs` |

Apply:
```bash
aws secretsmanager update-secret --secret-id brivva/env --secret-string '{...}'
aws ecs update-service --cluster brivva --service brivva --force-new-deployment
```

Truthy values: `1`, `true`, `yes`, `on` (case-insensitive). Anything
else — including unset — disables the switch.

Note: previous revisions of this runbook referenced `BRIVVA_DISABLE_TIER4`
and `BRIVVA_DISABLE_QWEN3`. Tier 4 dubbing and Qwen3 DashScope are not
present in the current server-rs tree, so those switches are intentionally
omitted. Re-add if/when those providers ship.

---

## Rollback — restore last-known-good in <2 min

### Automatic (preferred)

The Fargate service has the **ECS deployment circuit breaker** enabled.
If a new task-def revision fails to become healthy, ECS rolls back to
the previous revision automatically — you don't have to do anything.
Check CloudWatch or `aws ecs describe-services` for the rollback event.

### Manual Fargate rollback

Use the versioned script:

```bash
./scripts/rollback.sh --list   # show recent revisions with pinned image SHAs
./scripts/rollback.sh          # roll to the revision immediately before current
./scripts/rollback.sh 12       # roll to brivva:12 specifically
```

Each deploy pins the image by git SHA via `:{git-sha}` tag and registers
a new task-def revision, so every historical deploy is addressable. The
script confirms before acting and waits for `services-stable`.

### Workers rollback

```bash
cd workers && wrangler rollback
```

### Pages rollback

Redeploy prior commit from the Pages dashboard, or:

```bash
bunx wrangler pages deployment list --project-name=brivva
bunx wrangler pages rollback --project-name=brivva <DEPLOYMENT_ID>
```

### Rehearsal

Rehearse the Fargate path **before May 10**:
```bash
./scripts/rollback.sh --list       # verify you can read revisions
./scripts/rollback.sh <prev-rev>   # actually roll, then forward again
./scripts/smoke-test.sh prod       # confirm still healthy
./scripts/rollback.sh <new-rev>    # roll forward to latest
```

Practice makes 2am fast.

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

- [ ] Rehearse `./scripts/rollback.sh` once end-to-end (roll back, smoke-test, roll forward)
- [x] `scripts/smoke-test.sh` exists — verify it still passes against prod
- [x] Kill-switch env vars wired in server-rs (`BRIVVA_FALLBACK_TO_DEFAULT_VOICE`, `BRIVVA_FORCE_RTMP_NOT_RTMPS`)
- [x] ECS circuit breaker enabled — auto-rollback on failed deploys
- [ ] Bookmark CloudWatch dashboard URL on phone browser
- [ ] Have Simon's + MJ's phone numbers saved, reachable at 2am KST
- [ ] Keep a signed Tauri build on laptop as Plan B
- [ ] Test `aws logs tail` command works from phone hotspot (if home WiFi dies)
- [ ] Confirm `./scripts/rollback.sh --list` returns a useful history (needs a few prior deploys)
