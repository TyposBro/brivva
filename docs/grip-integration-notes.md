# Grip Live — Integration Notes (server-rs)

Operational notes for the Grip RTMPS target. Backs the Incident Response
entry in `ARCHITECTURE.md §Grip Live — Known Quirks` and the RTMP-drop
playbook in `docs/runbook.md §2`.

## URL shape

Grip mandates the explicit `:443` port on RTMPS. Default port parsing on
`rtmps://` URLs without an explicit port tends to resolve to 1935 in some
clients, which Grip rejects with TLS handshake failure, not a clean 4xx.

Canonical: `rtmps://live.grip.fans:443/live/<STREAM_KEY>`

Server-rs does not rewrite URLs. The Workers session bundle is responsible
for supplying the complete `rtmp_url` with port and `stream_key` separately;
`session_ws::start_rtmp_streams` joins them with a single `/`.

## FFmpeg flags

FFmpeg's `rtmps` protocol is `rtmp` over TLS — same muxer (`-f flv`), same
publish path.

**CRITICAL**: FFmpeg MUST use native RTMP/RTMPS with OpenSSL. The production
base image is built from source with `--enable-openssl`; CI asserts both
`ffmpeg -version | grep enable-openssl` and `ffmpeg -protocols | grep rtmps`.
Do not install or require Debian bookworm `librtmp`/rtmpdump in the runtime image.

The Dockerfile therefore uses a multi-stage build that compiles ffmpeg from
source with `--enable-openssl --enable-libx264 --enable-libx265
--enable-libmp3lame --enable-libopus --enable-libfreetype
--enable-libfontconfig --enable-libass`.

Flags that matter for Grip:

- `-f flv` — required muxer for RTMP family protocols. Already set.
- URL pass-through — FFmpeg negotiates TLS automatically when the URL
  starts with `rtmps://`. Nothing else to add on the publisher side.

Flags we deliberately did NOT add:

- `-rtmp_live live` — **input-side** option for consumers that need to
  disambiguate a live vs recorded playpath when pulling from an RTMP
  server. On the **publisher** side (our case) FFmpeg aborts with
  "Option rtmp_live not found." — smoke-tested against `mediamtx` and
  it broke the publish immediately. Documented here so nobody adds it
  back thinking it'll help.
- `-rtmp_buffer` / `-rtmp_flush_interval` — defaults are fine for the
  sizes we push. Tuning these without a reproducible failure is premature.
- `-tls_verify 0` — Grip's TLS cert is valid. Disabling verification
  would paper over a real MITM without diagnosing the actual handshake
  failure.

## Regional endpoint drops (~30 min)

Grip's regional RTMP endpoints close the connection after roughly 30
minutes, even when frames are flowing. FFmpeg exits with a non-zero code
and `Broken pipe` on stderr.

Recovery is automatic via `ffmpeg::spawn_health_monitor`:

1. `detect_crashed` observes `try_wait` returning `Some(status)`.
2. Up to `MAX_FFMPEG_RESTARTS` (3) respawns are attempted with a 2-second
   backoff between each.
3. Delay buffers (`StreamBuffers`) are reused across the restart so queued
   host media + TTS survive the FFmpeg lifecycle.

For shows longer than `MAX_FFMPEG_RESTARTS × ~30 min` we also rely on the
PTS-idle watchdog added for long-session support: if no host frames have
been pushed to a stream in `IDLE_RESTART_THRESHOLD` seconds we force a
restart pre-emptively, which catches both the Grip drop case and any
platform that silently stops accepting frames.

## Stream-key freshness

Grip stream keys contain a session auth token with an implicit TTL. Do
not cache them client-side beyond the session they were issued for. The
Workers session bundle should fetch a fresh key at session-create time.
If FFmpeg fails with an auth-shaped error during mid-show reconnect,
suspect an expired token — the session needs to be torn down and
recreated, not just reconnected.

## Kill-switch

If Grip's TLS stack is failing wholesale (rare but observed), set
`BRIVVA_FORCE_RTMP_NOT_RTMPS=1` and restart ECS. The switch is wired in
`features/broadcast/data/session_ws::start_rtmp_streams`; it rewrites
`rtmps://` → `rtmp://` before FFmpeg spawn. Only use on platforms that
accept unsecured RTMP — confirm with the platform owner first.

## Official Seller API (discovered 2026-04-19)

Grip has an **official Seller API** with `AccessKey` + `SecretKey` auth,
surfaced on Seller Center → profile → "Grip API (외부 연동)".
Support contact: `seller_support@gripcorp.co`.

Status 2026-04-20: Aziz emailed requesting the API spec. Reply pending.

If the spec covers the operations the paste-cred path reverse-engineers
(provision broadcast, fetch stream key, start, end, health), most of the
workarounds in this file retire post-May-10. Migration is gated behind
`BRIVVA_GRIP_USE_LEGACY` kill-switch so paste-cred remains instant
rollback. Not on the May 10 critical path — current paste-cred + native RTMP
flow is working e2e.

See `docs/may10-agent-tasks.md` Task 15 for the evaluation prompt.

## What we verified (2026-04-20)

- Live handshake against `live.grip.fans:443` from Fargate: **working**.
  600 frames pushed in 20s, broadcast flipped to `방송중` on Grip's end.
  This required ffmpeg compiled with native RTMP — see above.
- YouTube + Grip e2e in prod: **working**. Test users invited via Gmail
  through WhatsApp group. TikTok integration is the next target.

## What we did NOT verify

- Grip's undocumented stream-health API. As called out in
  `ARCHITECTURE.md`, we do not poll it. FFmpeg exit codes remain the
  authoritative signal. (Seller API may provide an official health
  endpoint — evaluate post-reply.)
