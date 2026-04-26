# WebRTC ingest migration plan

Status: **planned, not started**
Owner: _assign on kickoff_
Target: replace the host → server audio WebSocket path with WHIP/WebRTC
without touching the downstream STT / TTS / FFmpeg pipeline.

## Motivation

### Measured today

- Host → server ingest: **WebSocket binary frames** carrying 16-bit PCM
  mono @ 44.1 kHz, produced by `frontend/src/lib/AudioPipeline.ts` via
  `ScriptProcessorNode` (4096-sample buffers ≈ 93 ms per frame).
- Server ingest: `session_ws.rs` accepts each binary message and feeds
  `start_stt_pipelines(audio_rx)` in `pipeline/stt.rs`.
- Transport: TCP, HTTPS, through Cloudflare Tunnel.

### Problems this causes

1. **Latency floor is the buffer.** ScriptProcessor emits every ~93 ms,
   Soniox expects realtime streaming. We bleed ≥100 ms on the first hop
   before the pipeline even runs.
2. **TCP head-of-line blocking.** One dropped segment stalls every
   in-flight audio chunk. Pipeline starves → Soniox reconnects → user
   hears a hiccup.
3. **No native jitter handling.** Any wall-clock drift turns into
   crunchy audio in the mixed RTMP output.
4. **No packet loss concealment.** TCP retransmits and delivers late —
   worse for live audio than PLC would be.
5. **Uplink bandwidth is wasteful.** PCM s16 mono 44.1kHz = 705 kbps.
   Opus at the same perceptual quality = ~32 kbps.

### Target after this migration

- Host publishes Opus audio over WebRTC (SRTP over UDP).
- Server-rs exposes a WHIP endpoint, terminates the peer connection,
  decodes Opus → PCM s16le @ 44.1 kHz, feeds the **exact same**
  `mpsc::Sender<Vec<u8>>` that the WS path uses today.
- Expected ingest latency drop: **200–400 ms → 30–80 ms** (Opus frame
  size 20 ms + SRTP + Fargate network).
- Uplink bandwidth drop: **~705 kbps → ~32 kbps** per host.

## Scope

### In scope — step 1 (audio only)

- WHIP endpoint in `server-rs` at `POST /whip/session`.
  - Body: SDP offer from browser.
  - Response: SDP answer.
  - Auth: reuse existing JWT (same `JWT_SECRET`, same `sub` claim).
- Opus → PCM decode task that writes into the current `audio_rx`
  channel shape.
- Frontend `AudioPipeline.ts` replaced by an `RTCPeerConnection`
  wrapper.
- Smoke driver + stubs get a WHIP variant so CI validates the new
  path end-to-end.

### In scope — step 2 (video, later PR)

- Add a video transceiver to the WHIP offer.
- Server accepts H.264 (preferred, matches what ffmpeg emits already)
  or VP8.
- Replace the `face:frame` JSON-over-WS JPEG flow with WebRTC video.
- **Re-encode per target, not passthrough** — see "Caption burn-in
  compatibility" below. The tempting "WebRTC → RTMP copy" shortcut is
  not available to us.

### Out of scope

- Simulcast / SVC.
- TURN infrastructure. STUN via Google public servers is enough for
  Fargate (public IP) + most home networks.
- Server-side recording.
- Multi-host / viewer subscriptions.
- Android / iOS native clients.

## Architecture

### Current

```
browser (ScriptProcessor)
   │ PCM s16 44.1k mono, binary WS frames
   ▼
WebSocket /api/session                (session_ws.rs)
   │ mpsc::Sender<Vec<u8>>
   ▼
start_stt_pipelines (stt.rs)
```

### After step 1

```
browser (RTCPeerConnection, Opus 48k stereo/mono)
   │ SRTP over UDP (ICE, SDP via HTTP POST)
   ▼
WHIP POST /whip/session               (new: whip.rs)
   │ webrtc::peer_connection ≈ one task per host
   ▼
Opus depacketizer → Opus decoder → resample 48k→44.1k mono → PCM s16
   │ same mpsc::Sender<Vec<u8>> as WS path
   ▼
start_stt_pipelines (unchanged)
```

The WS path **stays up** alongside WHIP during step 1. The frontend
picks WebRTC by default but falls back to WS if `RTCPeerConnection`
setup fails or the host disables WebRTC via a flag.

### Face-frame video in step 1

Video still rides the old WS JSON `face:frame` messages until step 2.
Two options to keep the data+control plane working:

- **A:** frontend opens both a WebRTC peer (audio) and a WS (control +
  face frames). Simple. Two sockets per host.
- **B:** send face frames as binary `RTCDataChannel` messages on the
  same peer connection. One socket, but requires data-channel plumbing
  on the server too.

Recommendation: **A** for step 1. Cheaper to reason about. Delete the
WS entirely in step 2 when video moves to a media track.

## Caption burn-in compatibility

The current architecture burns translated captions into each target
RTMP output via ffmpeg's `drawtext` filter, reading `/tmp/subs-<lang>.txt`
with `reload=1`. This is per-target and per-language because every
stream needs different text.

**Burn-in forces a re-encode**, no matter what the ingest format is:
`drawtext` operates on decoded raw frames, so the pipeline is always
`decode → overlay → re-encode` for every target. The WebRTC "send
H.264 once, fan out to N RTMP outputs with `-c:v copy`" optimization
that some docs sell is **not available to this product** as long as
captions are burned in.

### What step 1 (audio) changes for burn-in

Nothing. Audio ingest → STT → TTS → caption-text write → drawtext
reload is untouched. The exact same per-language `subs-<lang>.txt`
files still feed the exact same per-target ffmpeg processes.
Migrating audio to WebRTC gives us the latency win without touching
the caption path at all.

### What step 2 (video) must preserve

Step 2 replaces the JPEG `face:frame` input to each ffmpeg with a
decoded H.264/VP8 frame source. The per-target ffmpeg invocation still
carries its own `drawtext=textfile=/tmp/subs-<lang>.txt:reload=1:…`
argument — that filter is **unchanged**. Only the video input side
changes.

Two candidate topologies for step 2:

1. **Single decode, per-target encode.** Server decodes the incoming
   WebRTC video once, produces raw frames on an internal broadcast
   channel, then each target's ffmpeg reads raw frames from stdin +
   applies `drawtext` + encodes H.264 + muxes to RTMP. One decode, N
   encodes. Caption-text files stay exactly where they are.
2. **Per-target decode+encode.** Each target ffmpeg gets the raw
   H.264 packets from the WebRTC track and does the full
   decode→drawtext→encode chain itself. N decodes, N encodes, simpler
   plumbing, more CPU.

Recommendation for step 2: **topology 1**. Linear in N on CPU for
encode (unavoidable), constant on decode, and keeps the per-target
ffmpeg command invocation almost identical to today. Burn-in
implementation survives untouched.

### Cost implication

This locks us into **~50–80% CPU per 1080p30 target** (current
baseline from `infra/README.md`). Step 2 does not make video cheaper,
it only makes video ingest cleaner. If we later want to skip burn-in
for platforms that accept text captions natively (YouTube, Facebook),
topology 1 still lets us drop `drawtext` on those specific target
ffmpegs and flip them to `-c:v copy` — **per-target, not global**.
That's a followup optimization, not part of this migration.

### Action items this affects

- Step 2 design doc (future, separate PR) must call out the "decode
  once, fan out raw frames" topology explicitly.
- The `core/audio/` resampler planned for step 1 establishes the
  pattern for a `core/video/` decoder + broadcast in step 2.
- `infra/README.md` "Sizing" table already assumes re-encode per
  target; no infra change needed.

## Server-side implementation

### Crate choice

Use `webrtc = "0.x"` from the webrtc-rs project. It is the only
production-grade pure-Rust WebRTC stack. Alternatives considered:

- `medea-jason` — client-focused.
- Pion via FFI — adds a Go runtime + CGO.
- libwebrtc via wrapper — binary size + build complexity unacceptable
  for Fargate slim image.

Cost: ~15 transitive crates, ~2 MB added to the release binary.

### New files

```
server-rs/src/features/broadcast/data/whip/
├── mod.rs          # route handler + JWT auth
├── peer.rs         # RTCPeerConnection lifecycle (accept offer → answer)
├── decoder.rs      # Opus RTP → PCM s16le 44.1k mono
└── session.rs      # glue: map peer → LiveSession + audio_tx
```

Keep the WS handler file `session_ws.rs` untouched. WHIP does not
reuse that module; it reuses the **domain** types (`LiveSession`,
`LiveSessions`, `SessionQuery`).

### Route wiring

`orchestration/router.rs`:

```rust
.route("/whip/session", post(whip_session_handler))
```

### Auth

Same JWT as WS: query string `?token=...` holds the HS256 JWT signed
by Workers. `whip::mod.rs` calls existing `data::auth::verify` and
rejects with 401 on failure. No new secrets.

### Peer lifecycle

1. Parse `Authorization: Bearer <jwt>` or `?token=` query (match WS
   contract for backward-compat; WHIP spec prefers the header).
2. Parse SDP offer from request body.
3. Build `RTCPeerConnection` with:
   - ICE servers: a single `stun:stun.l.google.com:19302`.
   - Single audio transceiver, `recvonly`, `codec=opus/48000/2`.
4. Set remote description → create answer → set local description.
5. `on_track` handler: for each incoming RTP packet, depacketize, push
   to decoder channel.
6. Return SDP answer in response body (`Content-Type:
   application/sdp`).
7. On `on_peer_connection_state_change == Closed | Failed`, drop the
   `LiveSession` entry the same way the WS handler does.

### Decoder task

One `tokio::spawn` per peer:

```rust
let mut opus = opus::Decoder::new(48_000, Channels::Mono)?;
loop {
    let rtp_pkt = rtp_rx.recv().await.unwrap();
    let pcm_48 = opus.decode(rtp_pkt.payload, …)?;
    let pcm_44 = resample_48k_to_44_1k_mono(&pcm_48);   // in core/
    audio_tx.try_send(pcm_44.to_le_bytes_vec());
}
```

- Opus decode: `opus = "0.3"` crate (libopus bindings, already ships
  in the FFmpeg base image so no new apt package).
- Resample: write a small linear resampler in `core/audio/` (44.1k is
  rational ratio 147/160 from 48k). Keep it pure for tests.
- Channel: reuse the existing `mpsc::channel::<Vec<u8>>(64)` from
  `session_ws.rs` — decoder task is a drop-in replacement for the WS
  binary-message handler that does `audio_tx.try_send(data.to_vec())`.

### Live session bookkeeping

The WS handler today does:

```
next_available_live_session_id → fetch_session_bundle → insert into
live_sessions → start FFmpeg per stream → spawn health monitor
```

The WHIP handler **does the same sequence**. Extract the shared
prelude into a private `spawn_live_session(state, user_id, source_lang,
session_id) -> LiveSessionHandle` helper inside the `broadcast/data`
module so both transports can call it. This is the only refactor of
existing code that this migration requires.

### Session teardown

WHIP peer close → `live_sessions.remove(id)` → stop ffmpeg, stop
health monitor, send status=ended to Workers. Mirror the existing WS
shutdown exactly.

## Frontend implementation

### Replace `AudioPipeline`

Rename `frontend/src/lib/AudioPipeline.ts` → `AudioPipelineWs.ts`
(kept as fallback) and add `AudioPipelineWebRtc.ts`:

```ts
class AudioPipelineWebRtc {
  private pc: RTCPeerConnection | null = null;
  private stream: MediaStream | null = null;

  async start(sessionId: string, token: string): Promise<void> {
    this.stream = await navigator.mediaDevices.getUserMedia({ audio: true });
    this.pc = new RTCPeerConnection({
      iceServers: [{ urls: "stun:stun.l.google.com:19302" }],
    });
    for (const track of this.stream.getAudioTracks()) {
      this.pc.addTrack(track, this.stream);
    }
    const offer = await this.pc.createOffer({ offerToReceiveAudio: false });
    await this.pc.setLocalDescription(offer);
    await this.waitForIceGathering(this.pc);

    const resp = await fetch(`${API_BASE}/whip/session?token=${encodeURIComponent(token)}&session_id=${sessionId}&source_lang=en`, {
      method: "POST",
      headers: { "Content-Type": "application/sdp" },
      body: this.pc.localDescription!.sdp,
    });
    if (!resp.ok) throw new Error(`WHIP ${resp.status}`);
    const answerSdp = await resp.text();
    await this.pc.setRemoteDescription({ type: "answer", sdp: answerSdp });
  }

  stop(): void {
    this.stream?.getTracks().forEach((t) => t.stop());
    this.pc?.close();
    this.pc = null;
    this.stream = null;
  }
}
```

### Selector in `useHostSession`

```ts
const useWebRtc = import.meta.env.VITE_INGEST === "webrtc" || true; // default on
const audio = useWebRtc ? new AudioPipelineWebRtc() : new AudioPipeline();
```

Kill-switch via env, no code change required to roll back.

### Mic analyser for the UI VU meter

Currently lives inside `AudioPipeline.ts`. Pull the `AnalyserNode`
creation into a shared helper since both transports need it.

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

- **Security group** `brivva-task` already allows all egress, and the
  cloudflared sidecar handles ingress on HTTP(S). WHIP is just HTTP
  POST so nothing changes there. But:
- **UDP for media.** RTCPeerConnection requires UDP between browser
  and server. Cloudflare Tunnel does **not** tunnel UDP by default.
  Two options:
  - Rely on **cloudflared udp-over-quic** if enabled (recent feature,
    check availability).
  - Use a **public ALB on 443/UDP (QUIC)** or raw UDP NLB. Bypass the
    tunnel for WebRTC.

  Recommendation: add a Network Load Balancer in `infra/main.tf` in a
  separate PR, give the task a dedicated public IP for UDP. Update
  `aws_security_group.task` to accept inbound UDP/3478 + UDP 50000–60000
  (ephemeral media ports).
- **New terraform var:** `whip_udp_port_range` default `"50000-60000"`.
- **ICE transport policy:** force `"relay"` is wasteful. Leave default
  `"all"` so host+srflx candidates are used. If we ever need TURN,
  spin up coturn on the same task and add UDP 3478 + TCP 443.

## Dependencies

| Item | Type | Notes |
|---|---|---|
| `webrtc = "0.x"` | crate | ~2 MB, ~15 transitive |
| `opus = "0.3"` | crate | requires libopus (already installed via ffmpeg) |
| `@peculiar/webrtc` or `wrtc` | npm, dev-only | for smoke driver |
| NLB + UDP security-group rules | terraform | separate PR after server code lands |

## Rollout

1. **Land server WHIP handler + WS still default** — ship to prod
   behind the existing WS. No user impact.
2. **Smoke WHIP job in CI** — validate server side in isolation.
3. **Frontend flag default off** — opt in via `?ingest=webrtc` URL
   param for internal testing.
4. **Stress test** — run two concurrent hosts for 30 min each, watch
   CPU + crash metrics.
5. **Flip default to WebRTC** — keep WS fallback code for a sprint.
6. **Delete WS audio path** — reclaim the 200 lines in `session_ws.rs`
   and retire `AudioPipelineWs.ts`.

Each step is independently reversible. No big-bang deploy.

## Risks and mitigations

| Risk | Mitigation |
|---|---|
| `webrtc` crate compile time bloats CI | cargo-chef layer already caches deps; one-time hit |
| Fargate network blocks inbound UDP | NLB PR before frontend flip; confirm with synthetic test |
| Opus decode adds CPU | measure before flip. Baseline ~3% per stream per FFmpeg core hint. Bump `task_cpu` if needed |
| Browser refuses WebRTC without HTTPS | prod already HTTPS via tunnel. Dev: run behind local cloudflared tunnel |
| Some corporate networks block UDP | WS path remains as fallback; frontend feature-detects |
| Session affinity breaks if we scale out | out of scope — single-task today. Revisit when autoscaling lands |

## Success criteria

- WHIP smoke CI job green for 7 consecutive nightly runs.
- Measured ingest latency (frontend `getUserMedia` timestamp → server
  `audio_tx` push) under 80 ms p95 on a staging stream.
- Zero ffmpeg crash-rate regression versus the WS baseline.
- No increase in the `WorkersStatusUpdateFailures` metric during the
  rollout window.

## Effort estimate

| Task | Hours |
|---|---|
| Extract `spawn_live_session` helper | 2 |
| WHIP route + peer lifecycle | 6 |
| Opus decode + resampler + unit tests | 4 |
| Frontend `AudioPipelineWebRtc` | 3 |
| Smoke driver + CI job | 3 |
| NLB + SG terraform + docs | 3 |
| Buffer for surprise (CPU tuning, ICE debug) | 5 |
| **Total** | **~26 h** |

Fits in 2 focused engineering days, or 3 calendar days with reviews
and stress testing.

## Open questions (before kickoff)

1. Do we want to keep WS audio forever as a fallback, or commit to
   WebRTC-only after the rollout window? Affects whether step 6 in
   rollout is a delete or a feature-flag freeze.
2. Does the team have a preference between sending face frames over
   `RTCDataChannel` vs the existing WS in step 1? Changes the number
   of connections per host.
3. Should we bundle a TURN server from day one for corporate-network
   users, or wait for a customer report?
4. Target browsers — do we care about Safari 16 (had WebRTC + WHIP
   quirks) or only modern evergreen?
5. Any compliance / SRTP encryption audit required before shipping?
6. For step 2: are we OK committing to "decode once, encode N times"
   as the video topology, or do we want the option to explore a
   no-burn-in path (platform-native captions) for the 2-3 platforms
   that support it? Affects the step 2 design scope.
