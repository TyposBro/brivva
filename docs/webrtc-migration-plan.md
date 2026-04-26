# WebRTC ingest migration plan

Status: **step 1 landed behind opt-in flag**
Owner: Aziz / Codex
Target: replace the host → server JPEG video path with WHIP/WebRTC H.264
while keeping the downstream STT / TTS / FFmpeg RTMP pipeline.

## Motivation

### Measured today

- Host video ingest: **base64 JPEG frames over WebSocket** (`face:frame`).
  Even after the 720p15 cap, this burns CPU in the browser and ships
  intra-only image frames instead of a real video codec.
- Server video ingest: `session_ws/messages.rs` decodes base64 JPEGs and
  feeds each FFmpeg child through `image2pipe`.
- Transport: TCP, HTTPS, through Cloudflare Tunnel.

### Problems this causes

1. **JPEG is not video.** It sends full compressed images every tick,
   so 4K30 would mean pushing 30 large JPEGs/sec through JS, base64, WS,
   Rust, and FFmpeg.
2. **No hardware video encoder.** Browsers already have efficient H.264
   encode paths for WebRTC; the JPEG path bypasses them.
3. **FFmpeg must re-encode JPEG input.** That keeps CPU pressure high
   even after subtitles were removed from scope.

### Target after this migration

- Host publishes H.264 video over WebRTC (SRTP over UDP).
- Server-rs exposes a WHIP endpoint, terminates the peer connection,
  depacketizes RTP H.264 into Annex B, and feeds the **same RTMP manager**
  that owns the FFmpeg children.
- FFmpeg stays because YouTube/TikTok/Grip ingest RTMP from this app.
  With WebRTC H.264, FFmpeg can copy video (`-c:v copy`) and mux it with
  each destination's audio instead of rendering/re-encoding video.

## Scope

### In scope — step 1 (video only, landed behind flag)

- WHIP endpoint in `server-rs` at `POST /whip/session`.
  - Body: SDP offer from browser.
  - Response: SDP answer.
  - Auth: reuse existing JWT (same `JWT_SECRET`, same `sub` claim).
- H.264 RTP depacketization into Annex B chunks.
- FFmpeg H.264 input mode that uses `-c:v copy`.
- Frontend `WebRtcVideoIngest` wrapper selected by `?ingest=webrtc` or
  `VITE_VIDEO_INGEST=webrtc`.
- Smoke driver + stubs get a WHIP variant so CI validates the new
  path end-to-end.

### In scope — step 2 (later PR)

- TURN/NLB hardening if direct Fargate UDP is unreliable.
- CI smoke test for WHIP video.
- Flip default from JPEG to WebRTC after production UDP is proven.

### Out of scope

- Simulcast / SVC.
- Server-side recording.
- Multi-host / viewer subscriptions.
- Android / iOS native clients.
- Captions/subtitles. Product scope is translated voice, not translated
  on-screen text.

## Architecture

### Current

```
browser (MediaRecorder-ish manual canvas)
   │ base64 JPEG JSON messages
   ▼
WebSocket /api/session                (session_ws.rs)
   │ RtmpManager::push_video_frame
   ▼
FFmpeg image2pipe -> libx264 -> RTMP
```

### After step 1

```
browser (RTCPeerConnection, H.264 video)
   │ SRTP over UDP (ICE, SDP via HTTP POST)
   ▼
WHIP POST /whip/session               (webrtc_ingest.rs)
   │ webrtc::peer_connection ≈ one task per host
   ▼
H.264 RTP depacketizer → Annex B chunks
   │ RtmpManager::push_h264_annex_b
   ▼
FFmpeg h264 stdin -> -c:v copy -> RTMP
```

The WS path **stays up** alongside WHIP during step 1 for audio,
control messages, and the default JPEG fallback. WebRTC video is opt-in
with `?ingest=webrtc` or `VITE_VIDEO_INGEST=webrtc` until UDP is proven
in production. Production now opens a bounded Fargate UDP media range
and uses STUN to gather public ICE candidates.

### Why captions are gone

Brivva's product surface is translated/cloned voice, not translated
text. Removing captions makes WebRTC valuable immediately: H.264 from
the browser can be copied into RTMP outputs instead of decoded,
rendered with `drawtext`, and re-encoded once per language. This keeps
quality higher and CPU lower. Captions/native subtitles remain out of
scope unless a paying B2B customer explicitly requires them.

## Server-side implementation

### Crate choice

Use `webrtc = "0.x"` from the webrtc-rs project. It is the only
production-grade pure-Rust WebRTC stack. Alternatives considered:

- `medea-jason` — client-focused.
- Pion via FFI — adds a Go runtime + CGO.
- libwebrtc via wrapper — binary size + build complexity unacceptable
  for Fargate slim image.

Cost: ~15 transitive crates, ~2 MB added to the release binary.

### Implemented files

```
server-rs/src/features/broadcast/data/webrtc_ingest.rs
frontend/src/features/broadcast/presentation/webrtc-video-ingest.ts
```

The WS handler remains the compatibility path and still owns audio,
STT/TTS, control messages, and default JPEG video.

### Route wiring

`orchestration/router.rs`:

```rust
.route("/whip/session", post(whip_session_handler))
```

### Auth

Same JWT as WS: query string `?token=...` holds the HS256 JWT signed
by Workers. `webrtc_ingest.rs` calls existing `data::auth::verify` and
rejects with 401 on failure. No new secrets.

### Peer lifecycle

1. Parse `?token=` query to match the existing WS contract.
2. Parse SDP offer from request body.
3. Build `RTCPeerConnection` with:
   - bounded ICE UDP ports matching the Fargate security group.
   - configured STUN URLs for public server-reflexive candidates.
   - incoming H.264 video track from the browser.
4. Set remote description → create answer → set local description.
5. `on_track` handler: for each incoming RTP packet, depacketize H.264
   to Annex B and push to `RtmpManager::push_h264_annex_b`.
6. Return SDP answer in response body (`Content-Type:
   application/sdp`).
7. Keep the peer alive in a long-running task. The WS remains
   responsible for full session teardown today.

### Live session bookkeeping

The WS handler today does:

```
next_available_live_session_id → fetch_session_bundle → insert into
live_sessions → start FFmpeg per stream → spawn health monitor
```

The WHIP handler does **not** create a second live session. It attaches
to the existing WS-created session by Workers `sessionId`. That keeps
audio/STT/TTS/control behavior stable while only replacing video.

### Session teardown

WS close still tears down the session. WHIP close only stops the video
peer. This is intentional until WebRTC owns audio/control too.

## Frontend implementation

### Add `WebRtcVideoIngest`

The audio `AudioPipeline` remains unchanged. The new video path starts
a peer connection against the already-open WS session:

```ts
class WebRtcVideoIngest {
  private pc: RTCPeerConnection | null = null;

  async start(stream: MediaStream, sessionId: string, token: string) {
    this.pc = new RTCPeerConnection();
    this.pc.addTrack(stream.getVideoTracks()[0], stream);
    const offer = await this.pc.createOffer();
    await this.pc.setLocalDescription(offer);
    // POST local SDP to /whip/session...
  }
}
```

### Selector in `useHostSession`

```ts
const useWebRtc = appConfig().videoIngest === "webrtc";
if (useWebRtc) params.videoMode = "webrtc-h264";
```

Default remains JPEG. Opt in with `?ingest=webrtc` or
`VITE_VIDEO_INGEST=webrtc`.

## Smoke + E2E coverage

### Smoke driver

Add `tests/e2e/driver_whip.ts` alongside the existing driver:

1. Mint JWT same way.
2. Load `wrtc` npm package (Node WebRTC binding) or use
   `@peculiar/webrtc` — both polyfill `RTCPeerConnection` for Bun.
3. `getUserMedia` → replace with `MediaStreamTrack.fromCanvas` +
   sine-wave via `AudioNode` playing into a `MediaStreamTrack`.
4. POST SDP to `/whip/session`, set remote description.
5. Let it run 40 s, close peer.
6. Assert ffprobe on `rtmp://localhost:1935/live/smoke-ja` as today.
7. Assert it saw `translation` + `tts_end` events over the existing
   control WS.

CI workflow `smoke.yml` gains a second job `smoke-whip` that runs the
WHIP driver against the same compose stack. Both must be green.

### Full E2E (future, out of scope here)

Covered by task D once this migration is stable.

## Infra changes

- **Security group** `brivva-task` still keeps HTTP private to the
  cloudflared sidecar, but now opens only the configured WebRTC UDP
  media range.
- **UDP for media.** RTCPeerConnection requires UDP between browser
  and server. Cloudflare Tunnel handles the WHIP HTTP POST; Terraform
  opens direct Fargate UDP ports with `webrtc_udp_port_min=50000` and
  `webrtc_udp_port_max=50100`.
- **STUN.** Terraform injects `BRIVVA_WEBRTC_STUN_URLS`
  (`stun:stun.l.google.com:19302` by default), and server-rs includes
  those URLs in the peer connection so the SDP answer can carry a public
  server-reflexive candidate.
- **ICE transport policy:** force `"relay"` is wasteful. Leave default
  `"all"` so host+srflx candidates are used. If we ever need TURN,
  spin up coturn on the same task and add UDP 3478 + TCP 443.

## Dependencies

| Item | Type | Notes |
|---|---|---|
| `webrtc = "0.x"` | crate | ~2 MB, ~15 transitive |
| `@peculiar/webrtc` or `wrtc` | npm, dev-only | for smoke driver |
| bounded UDP security-group rules | terraform | direct Fargate UDP media path |

## Rollout

1. **Land server WHIP handler + WS still default** — ship to prod
   behind the existing WS. No user impact.
2. **Smoke WHIP job in CI** — validate server side in isolation.
3. **Frontend flag default off** — opt in via `?ingest=webrtc` URL
   param for internal testing.
4. **Stress test** — run two concurrent hosts for 30 min each, watch
   CPU + crash metrics.
5. **Flip default to WebRTC** — keep WS fallback code for a sprint.
6. **Decide whether audio moves later** — separate from this video
   migration.

Each step is independently reversible. No big-bang deploy.

## Risks and mitigations

| Risk | Mitigation |
|---|---|
| `webrtc` crate compile time bloats CI | cargo-chef layer already caches deps; one-time hit |
| Fargate network blocks inbound UDP | NLB PR before frontend flip; confirm with synthetic test |
| Browser negotiates VP8 instead of H.264 | frontend prefers H.264; server ignores non-H.264 payloads until VP8 transcode is explicitly implemented |
| Browser refuses WebRTC without HTTPS | prod already HTTPS via tunnel. Dev: run behind local cloudflared tunnel |
| Some corporate networks block UDP | WS path remains as fallback; frontend feature-detects |
| Session affinity breaks if we scale out | out of scope — single-task today. Revisit when autoscaling lands |

## Success criteria

- WHIP smoke CI job green for 7 consecutive nightly runs.
- Measured video ingest latency under 120 ms p95 on a staging stream.
- Zero ffmpeg crash-rate regression versus the WS baseline.
- No increase in the `WorkersStatusUpdateFailures` metric during the
  rollout window.

## Effort estimate

| Task | Hours |
|---|---|
| WHIP route + peer lifecycle | 6 |
| H.264 depacketize + FFmpeg copy mode | 4 |
| Frontend `WebRtcVideoIngest` | 3 |
| Smoke driver + CI job | 3 |
| NLB + SG terraform + docs | 3 |
| Buffer for surprise (CPU tuning, ICE debug) | 5 |
| **Total** | **~26 h** |

Initial implementation is landed; remaining work is infra, smoke
coverage, and production soak.

## Open questions (before kickoff)

1. Do we want to keep WS audio forever as a fallback, or move audio to
   WebRTC in a later migration?
2. Do we want TURN or NLB first for production UDP?
3. Should we bundle a TURN server from day one for corporate-network
   users, or wait for a customer report?
4. Target browsers — do we care about Safari 16 (had WebRTC + WHIP
   quirks) or only modern evergreen?
5. Any compliance / SRTP encryption audit required before shipping?
