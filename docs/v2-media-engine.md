# RFC: Brivva V2 Multilingual Media Engine

Status: draft / architecture-only / implementation frozen pending review
Owner: Aziz

## Accepted V2 decisions for implementation planning

These are the review defaults unless Aziz explicitly changes them before implementation starts.

- **First implementation task:** timeline shadow mode only. It must emit logs/metrics only and must not change FFmpeg drains, WebRTC ingest behavior, output pacing, or publishing.
- **Audio path:** WebRTC Opus with RTP timestamps is the preferred V2 path; timestamped WS PCM is allowed as a bridge/fallback; old raw PCM remains accepted until burn-in passes.
- **Health persistence:** Phase 1 is logs-only. D1 latest-health snapshots wait until operator UI/control needs them in Phase 3.
- **Operator controls:** no UI, D1 command queue, or runtime control surface before Phase 3; command vocabulary can be defined earlier as docs/types only.
- **Render graph:** wrap current per-output FFmpeg first; shared decode and GPU worker split are later phases, not prerequisites for V2 health/control.
- **Shared decode prototype:** defer FFmpeg complex graph vs GStreamer until after render-graph adapter proof.
- **Billing/degraded minutes:** business/ops policy required before paid V2 launch, but it must not block Phase 1 shadow metrics. Early recommendation remains free/non-billed degraded minutes during launch incidents.

## Steering rule

Major implementation is paused for the next architecture pass. Do not rewrite the live path, FFmpeg pipeline, WebRTC ingest, or deployment path yet. Code changes during this pass must be small, pure, reversible domain primitives/tests that clarify the RFC.

No more production code should be added until this RFC is complete and reviewed. Documentation, diagrams, and tests-only scaffolding are allowed. A prior behavior-equivalent edit to `session_ws/webrtc.rs` was reverted because it touched the live WebRTC ingest path.

## Current change classification

### Pre-existing unrelated changes — exclude from V2 review

These files are in the working tree but are not part of this RFC packet:

- `frontend/.pi-lens/turn-state.json`
- `frontend/playwright.real-stack.config.ts`
- `workers/src/orchestration/app.ts`
- `scripts/test-local-nvenc-youtube.sh`
- `server-rs/.pi-lens/turn-state.json`

### V2 RFC/scaffolding changes — allowed before review

- `docs/v2-media-engine.md` — architecture/RFC only.
- `server-rs/src/features/broadcast/domain/media_timeline.rs` — isolated pure timeline/jitter vocabulary + tests.
- `server-rs/src/features/broadcast/domain/render_graph.rs` — isolated pure render/health/degradation vocabulary + tests.
- `server-rs/src/features/broadcast/domain/mod.rs` — module exports for the isolated scaffolding above.

### Live-path changes — currently none intended

- `server-rs/src/features/broadcast/data/session_ws/webrtc.rs` was reverted to avoid live ingest behavior changes before RFC review.
- No FFmpeg, WebRTC ingest, deployment, Workers runtime, or frontend runtime path should change in this RFC packet.

## Product outcome

One host stream produces 2–4 localized broadcast outputs in parallel. Each output has independent language, subtitles, translated/TTS audio, logo, product CTA, lower-third, destination, health, and restart/kill controls.

Output identity must be stable and unambiguous. Draft format:

```txt
output_id = {session_id}:{lang}:{destination_platform}:{index}
```

Examples: `B8EF28:ja:youtube:0`, `B8EF28:ja:youtube:1`, `B8EF28:ko:tiktok:0`.

The `index` is required because one session may publish multiple outputs with the same language/platform but different stream keys, overlays, or destinations.

```txt
Host browser
  ↓ WebRTC A/V with source timestamps
Media ingest session
  ↓ authoritative media clock
  ↓ A/V jitter buffers
  ↓ source decode/normalize once
Render graph
  ├─ ja / YouTube: subtitles + TTS + logo + CTA → encode → RTMP
  ├─ zh / Grip:    subtitles + TTS + logo + CTA → encode → RTMP
  ├─ ko / TikTok:  subtitles + TTS + logo + CTA → encode → RTMP
  └─ source/pass:  logo + CTA only                     → encode → RTMP
```

## Current-state map

| Area              | Current implementation                                     | Strength                                               | V2 gap                                                          |
| ----------------- | ---------------------------------------------------------- | ------------------------------------------------------ | --------------------------------------------------------------- |
| Control plane     | Workers + D1 own users, sessions, streams, credentials     | Good separation from media hot path                    | Needs per-output health/control schema later                    |
| Host video ingest | WebRTC H.264 video track in `session_ws/webrtc.rs`         | Browser-native timestamped video already exists        | Clock/jitter model only partly explicit                         |
| Host audio ingest | Raw PCM frames over host WebSocket                         | Stable launch fallback                                 | Arrival-time based; no RTP/sample PTS                           |
| Media clock       | RTP video timestamp maps to server `Instant`               | Preserves video cadence better than wall-clock arrival | No shared A/V timeline yet                                      |
| Buffering         | Per-output delay buffers inside `RtmpManager`              | Output delays independent per language                 | Delay buffer is not a formal jitter buffer with health stats    |
| STT/translation   | Soniox pipeline emits source/final/translated text         | Already supports multiple target languages             | Subtitle timing is utterance/event based, not timeline based    |
| TTS               | Per-language TTS workers enqueue PCM into output queue     | Slow TTS isolated from Soniox loop                     | TTS PCM has no target PTS; drift not observable                 |
| Video output      | FFmpeg child per stream, re-encodes H.264 to RTMP          | Failure isolation already exists per destination       | Decodes/re-encodes per output; no shared decode/normalize node  |
| Restart policy    | FFmpeg health monitor restarts crashed/idle stream         | One destination crash does not inherently end session  | No operator API for one-output restart/kill yet                 |
| GPU path          | Laptop RTX + ECS `brivva-gpu` blue/green NVENC path exists | Deployment foundation is ready                         | Not wired as render-worker graph yet                            |
| Observability     | FFmpeg speed/drop logs, metrics reporter                   | Launch useful                                          | Missing jitter depth, A/V drift, subtitle lag, per-output state |

## Code-reality audit for RFC review

This section is intentionally documentation-only. It pins which current code paths the RFC is describing so review can catch drift before implementation starts.

| RFC claim                                 | Current evidence to review                                                                                                  | Implementation implication                                                    |
| ----------------------------------------- | --------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------- |
| Video is already timestamped              | `server-rs/src/features/broadcast/data/session_ws/webrtc.rs` handles RTP packets and maps RTP timestamps to a media instant | V2 should shadow/extend this path first, not replace it blindly               |
| Audio is not timestamped                  | `session_ws` binary messages feed PCM bytes into STT/audio buffers without sample-clock metadata                            | Timestamped audio is the first real sync unlock                               |
| Output isolation partly exists            | `RtmpManager` owns one FFmpeg child/buffers/restart count per stream                                                        | Per-output health/control can wrap this before render-graph rewrite           |
| Current video still re-encodes per output | `ffmpeg/args.rs` builds a re-encode path for RTMP stability/NVENC                                                           | Shared decode is an optimization phase, not a prerequisite for V2 control     |
| Metrics exist but not per-output enough   | `SessionMetrics` reports source/output seconds; FFmpeg stderr logs speed/drop                                               | Health RFC must add output id, drift, jitter, subtitle lag, restart state     |
| GPU deployment foundation exists          | `infra/README.md` documents `brivva-gpu`, NVENC, zero-spend guard, rehearsal runner                                         | Do not create a new GPU deployment architecture until in-process proof passes |
| Older WebRTC migration doc exists         | `docs/webrtc-migration-plan.md` is audio/WHIP oriented and predates the current H.264 video path                            | Use it as background only; this RFC is the source of truth for V2 sequencing  |

Review checklist before implementation:

- Confirm every file path above still matches current code.
- Confirm no RFC phase assumes a non-existent endpoint/schema/script.
- Confirm current V1 smoke/rehearsal commands still work before adding V2 flags.
- Confirm docs do not require a DB migration or deploy path change before RFC approval.

### Current-state verification evidence — 2026-05-01 reflection pass

Documentation-only audit commands were run with `grep`; no production files were edited.

| Claim checked                                                   | Evidence observed                                                                                                                 | RFC impact                                                              |
| --------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------- |
| Video RTP path exists and is still live-path code               | `session_ws/webrtc.rs` contains `track.read_rtp()`, `VideoRtpClock`, and `push_video_h264_at(...)`                                | Any future clock refactor here is live-path and must wait for review    |
| Live-path WebRTC edit is reverted                               | `git diff -- server-rs/src/features/broadcast/data/session_ws/webrtc.rs` is empty                                                 | RFC packet has no WebRTC ingest behavior change                         |
| Audio PCM path is still WS binary                               | `session_ws/mod.rs` handles `Message::Binary`; `messages.rs` sends bytes to STT and `push_host_audio(&data)`                      | Timestamped audio remains a future phase, not current behavior          |
| Output isolation exists today                                   | `ffmpeg/mod.rs` has per-stream `RtmpStream`, `restart_count`, `detect_crashed`, `restart_stream`, and `spawn_health_monitor`      | Phase 3/4 should wrap current isolation before replacing it             |
| Current metrics are session/lang-level, not output-health-level | `SessionMetrics` snapshots `source_seconds` and `output_seconds_by_lang`                                                          | Per-output health payload is additive; do not overload billing metrics  |
| GPU foundation exists already                                   | `infra/README.md` documents `brivva-gpu`, `run-ecs-gpu-rehearsal.sh`, `check-gpu-zero-spend.sh`, and `BRIVVA_VIDEO_ENCODER=nvenc` | V2 should reuse this path; no new deployment architecture in RFC packet |

## Relationship to `docs/webrtc-migration-plan.md`

`docs/webrtc-migration-plan.md` is useful history, not the V2 source of truth. It was written for an audio-only WHIP migration and predates the current H.264 WebRTC video ingest path.

| Topic           | Older WebRTC migration plan                          | V2 RFC decision                                                                        |
| --------------- | ---------------------------------------------------- | -------------------------------------------------------------------------------------- |
| Scope           | Audio-only WHIP first, downstream unchanged          | Multilingual production engine: clock, jitter, render graph, per-output health/control |
| Signaling       | New `POST /whip/session` endpoint                    | No new endpoint before RFC review; current WS-signaled WebRTC video stays baseline     |
| Video state     | Future step, replacing old face-frame path           | Current code already has WebRTC H.264 video; V2 shadows/extends it after review        |
| Audio path      | WebRTC Opus → decode/resample → existing PCM channel | Still preferred, but timestamped WS PCM remains explicit fallback/bridge               |
| Infra           | NLB/UDP/TURN decisions tied to WHIP                  | Reuse existing GPU/WebRTC UDP foundation first; no infra change in RFC packet          |
| Rollout         | Flip frontend ingest default after WHIP smoke        | Phase-gated flags; no production-code work until RFC reviewed                          |
| Source of truth | Historical plan                                      | Superseded by this RFC for V2 sequencing                                               |

Conflicts resolved for now:

- Do **not** implement `/whip/session` as Phase 1 just because the older doc says so.
- Do **not** add NLB/TURN/Terraform changes before timeline shadow and RFC review.
- Do **not** delete or replace the current host WS; it remains control + PCM fallback.
- Do **not** touch `session_ws/webrtc.rs` again until the RFC review accepts the first implementation task.

Open question carried forward: WebRTC Opus vs timestamped PCM first. The older doc argues strongly for Opus; this RFC keeps the decision explicit because current video ingest already moved ahead and launch risk now matters more than theoretical transport purity.

## Target architecture

### Logical graph

```txt
SourceIngestNode
  ├─ WebRtcVideoTrack(H264/RTP timestamps)
  └─ WebRtcAudioTrack(Opus/RTP timestamps) OR TimestampedPcmFallback

MediaTimelineNode
  ├─ maps RTP/sample clocks → session media PTS
  ├─ owns authoritative playhead
  └─ emits sync/drift stats

JitterBufferNode
  ├─ video jitter buffer
  └─ audio jitter buffer

DecodeNormalizeNode
  ├─ decodes source video once
  ├─ normalizes to target canvas/FPS
  └─ publishes frames to render graph

OutputPipelineNode × N
  ├─ SubtitleNode(translated timed cues)
  ├─ TtsAudioNode(translated PCM aligned to cue/media time)
  ├─ OverlayNode(logo/product CTA/lower-third)
  ├─ AudioMixNode(host + TTS + ducking)
  └─ EncodePublishNode(FFmpeg/GStreamer → RTMP)
```

### Deployment graph

Phase 1 stays in-process inside `server-rs`. Only after proof:

```txt
server-rs ingest/control session
  ↓ internal IPC/queue (future)
GPU media worker(s)
  ↓ RTMP per output
platforms
```

Do not introduce worker boundaries before the in-process graph is observable and testable.

## Phased migration plan

### Phase 0 — RFC and pure contracts

Goal: no live-path behavior change.

- Document current state, target graph, failure modes, rollback.
- Add pure timeline/render graph/health types where they clarify terms.
- Unit-test policy decisions, not media processing.

Exit: architecture reviewed; tests compile; current live path unchanged.

### Phase 1 — timeline shadow mode

Goal: compute V2 timing in parallel without driving output.

- Keep current FFmpeg/WebRTC path active.
- Add `BRIVVA_V2_TIMELINE_SHADOW=1` to calculate video media PTS and jitter depth from current RTP video.
- For audio in Phase 1, log only approximate arrival-time/queue timing because current raw PCM has no authoritative RTP/sample PTS yet. Do not claim true A/V drift until Phase 2 timestamped audio exists.
- Log shadow metrics only. Do not feed drains from new buffers.

Exit: 30–60 min stream shows stable video timing/jitter metrics, approximate audio timing is clearly labeled as non-authoritative, and there is no production behavior change.

#### Phase 1 implementation evidence — 2026-05-01

Implemented as logs-only shadow mode behind `BRIVVA_V2_TIMELINE_SHADOW`:

- Flag source: `AppConfig::from_env()` reads `BRIVVA_V2_TIMELINE_SHADOW`; `BroadcastState::new()` defaults it to `false`.
- Video shadow hook: `session_ws/webrtc.rs` computes RTP media PTS/jitter from the existing H.264 RTP packets and emits `v2 timeline shadow video` logs only when the flag is enabled.
- Audio shadow hook: `session_ws/messages.rs` emits `v2 timeline shadow audio` logs only when the flag is enabled; payload kind is `audio_arrival_non_authoritative`, includes `authoritative: false`, and explicitly notes Phase 1 audio timing is approximate arrival/queue timing only.
- No FFmpeg drain/output pacing hook was added; existing `push_video_h264_at(...)` and `push_host_audio(...)` calls remain the live behavior path.
- Focused proof: `cargo test -p server-rs media_timeline`, `cargo test -p server-rs timeline_shadow`, and config/state flag tests passed.

Rollback: disable `BRIVVA_V2_TIMELINE_SHADOW`; with the flag off, shadow state is not allocated and no shadow logs are emitted.

#### Phase 1 runtime proof — 2026-05-01

Accepted Phase 1 and ran fake-browser-media rehearsal automation:

```bash
./scripts/prove-v2-timeline-shadow.sh --duration 120 --rollback-duration 45 --source ko
./scripts/prove-v2-timeline-shadow.sh --duration 600 --rollback-duration 45 --source ko
```

Observed result:

- ✅ Short shadow-on rehearsal: 2 minutes, Playwright Chromium fake camera/mic, local RTMP sink, source `ko`, passthrough output.
- ✅ Full shadow-on rehearsal: 10 minutes with the same fake-media/local-RTMP path.
- ✅ Logs contained `v2 timeline shadow video` and `video_rtp_authoritative` payloads.
- ✅ Logs contained `v2 timeline shadow audio` and `audio_arrival_non_authoritative` payloads.
- ✅ Rollback smoke: 45 seconds with `BRIVVA_V2_TIMELINE_SHADOW=0`.
- ✅ Rollback logs did not contain video/audio shadow events or payload kinds.
- ✅ No FFmpeg/RTMP/pipeline diff was part of the implementation packet.

Proof directories:

- `.dev-logs/v2-timeline-shadow-proof/20260501-153746/`
- `.dev-logs/v2-timeline-shadow-proof/20260501-154159/`

Automation note: `scripts/prove-v2-timeline-shadow.sh` preserves phase-specific proof logs as `shadow-on-server-rs.log`, `shadow-on-dev-stack.log`, `rollback-server-rs.log`, and `rollback-dev-stack.log` under `.dev-logs/v2-timeline-shadow-proof/<timestamp>/`.

### Phase 2 — timestamped audio ingest

Goal: remove arrival-time audio ambiguity.

Preferred path: WebRTC audio track with RTP timestamps.
Fallback path: WS PCM frame includes `sample_index`, `sample_rate`, and client monotonic capture time.

- Keep old PCM frame format accepted.
- Add feature flag `BRIVVA_V2_TIMESTAMPED_AUDIO=1` for new timing.
- STT can still consume PCM immediately; output mix aligns to media PTS.

Exit: audio/video drift measurable and bounded; old clients still work.

### Phase 3 — output health/control surface

Goal: operator can manage one output without ending the session.

- Define per-output health snapshot.
- Add control commands: restart output, stop output, set captions-only/default-voice/no-overlays.
- Surface in logs first, then Workers/D1/UI.

Exit: simulated TTS/RTMP failure affects only one output in tests and manual rehearsal.

### Phase 4 — in-process render graph

Goal: model outputs as nodes while still using current FFmpeg process model.

- Wrap current `RtmpManager` streams as `EncodePublishNode` adapters.
- Add subtitle/overlay layer specs, but gate actual render composition.
- Keep per-output FFmpeg until shared decode is proven.

Exit: graph controls current outputs without broad rewrites.

### Phase 5 — shared decode/normalize

Goal: decode source video once, fork frames to outputs.

- Choose FFmpeg filter graph vs GStreamer vs Rust-side frame graph after prototype.
- Validate 2 outputs first, then 4.
- NVENC on laptop/ECS required for heavy paths.

Exit: one decode, N output encodes, sync metrics green.

### Phase 6 — GPU worker split

Goal: move heavy graph to ECS GPU worker only after in-process success.

- Reuse existing `brivva-gpu` service, zero-spend guard, endpoint smoke, proof collector.
- Keep Fargate/laptop fallback hot.
- Worker split must preserve operator controls and health events.

Exit: ECS GPU 4-output burn-in 60–90 min with proof archive.

## Failure-mode matrix

| Failure                             | Desired behavior                              | Health state                                       | Operator action                                      | Rollback/fallback                                                |
| ----------------------------------- | --------------------------------------------- | -------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------------------- |
| TTS API slow/fails for one language | Captions continue; other languages live       | output `degraded`, degradation `captions_only`     | force default voice / captions-only / restart output | `BRIVVA_FALLBACK_TO_DEFAULT_VOICE=1` or disable translated audio |
| Voice clone bad/accent drift        | Use default voice for affected output         | output `degraded`, degradation `default_voice`     | force default voice per output                       | global default voice kill-switch                                 |
| Subtitle renderer fails             | Audio/video continue without burned subtitles | output `live` or `degraded`                        | disable subtitle layer                               | return to V1 no-overlay output                                   |
| Overlay renderer fails              | Output continues without logo/CTA/lower-third | output `degraded`, degradation `overlays_disabled` | disable overlay layer                                | no-overlay profile                                               |
| One RTMP destination rejects/drops  | That output restarts/fails; session continues | output `restarting` then `failed/stopped`          | restart/stop one output                              | downgrade RTMPS or remove destination                            |
| Encoder below realtime              | Output restarts or lowers profile             | output `degraded/restarting`                       | lower quality / restart                              | CPU/x264 fallback or fewer outputs                               |
| GPU worker dies                     | Affected outputs restart on fallback worker   | outputs `restarting`                               | fail to laptop/Fargate path                          | set media URL back to V1 engine                                  |
| Media clock drift grows             | Alert before audience notices                 | session/output `degraded` with drift_ms            | restart output or session                            | disable V2 timeline driver                                       |
| Jitter buffer overflow              | Drop late/oldest frames, keep live            | jitter `dropped_overflow` increments               | reduce quality/outputs                               | V1 delay buffers                                                 |

## Observability contract

Minimum per-output health snapshot:

```json
{
  "session_id": "...",
  "output_id": "ja-youtube",
  "lang": "ja",
  "destination": "youtube",
  "state": "live|degraded|restarting|failed|stopped",
  "degradation": "none|captions_only|default_voice|overlays_disabled|disabled",
  "media_clock_ms": 123456,
  "video_pts_ms": 123420,
  "audio_pts_ms": 123410,
  "audio_drift_ms": -10,
  "subtitle_lag_ms": 180,
  "jitter_depth_ms": { "audio": 80, "video": 66 },
  "drops": { "late": 0, "overflow": 0 },
  "encode": { "fps": 30.0, "speed": 1.02, "drop_frames": 0 },
  "queues": { "tts_bytes": 88200, "video_frames": 3 },
  "restart_count": 0,
  "last_write_age_ms": 45
}
```

Logging rules:

- Every degradation decision logs `session_id`, `output_id`, `failure_kind`, `action`.
- Every operator command logs requested/accepted/completed states.
- Every automatic restart logs prior health snapshot and restart count.
- Metrics are safe to emit before Workers/D1 schema exists; logs are enough for Phase 1.

## Rollback plan

### Behavior flags

V2 behavior stays behind explicit flags:

- `BRIVVA_V2_TIMELINE_SHADOW=1` — calculate/log only.
- `BRIVVA_V2_TIMESTAMPED_AUDIO=1` — use timestamped audio path.
- `BRIVVA_V2_OUTPUT_CONTROLS=1` — accept per-output controls.
- `BRIVVA_V2_RENDER_GRAPH=1` — route output through graph adapter.
- `BRIVVA_V2_SHARED_DECODE=1` — enable shared decode/fan-out.
- `BRIVVA_V2_GPU_WORKERS=1` — use ECS GPU worker path.

Rollback order:

1. Disable newest V2 flag.
2. Restart only affected output if possible.
3. Restart media session if output restart fails.
4. Repoint frontend `VITE_MEDIA_URL` to known-good V1 engine/laptop fallback.
5. Roll back ECS task definition/image only if V1 path also regressed.

### Hard no-go gates

Do not enable next phase if any are true:

- Cannot prove old V1 path still passes smoke.
- Cannot identify failing output within 30s from logs.
- One-output failure can still kill whole session.
- Drift/jitter metrics are missing or untrusted.
- Rollback requires code deploy instead of env/config switch.

## Decision matrices

### Timestamped audio path

| Option                       | Pros                                                                 | Cons                                                    | RFC default               |
| ---------------------------- | -------------------------------------------------------------------- | ------------------------------------------------------- | ------------------------- |
| WebRTC Opus audio track      | Real RTP timestamps, lower bandwidth, browser-native jitter handling | Requires decode/resample path and UDP reliability proof | Preferred V2 path         |
| Timestamped WS PCM envelope  | Minimal media-stack change; keeps current audio decoder-free         | Still TCP HOL blocking; custom clock drift handling     | Fallback / bridge path    |
| Keep current raw PCM forever | Lowest implementation risk                                           | Cannot prove A/V sync or drift; high bandwidth          | No — launch fallback only |

Decision: design for WebRTC audio, but keep timestamped PCM fallback and old raw PCM compatibility until burn-in passes.

### Shared video render graph

| Option                                | Pros                                                                    | Cons                                                                  | RFC default                         |
| ------------------------------------- | ----------------------------------------------------------------------- | --------------------------------------------------------------------- | ----------------------------------- |
| FFmpeg complex filter graph           | Uses known FFmpeg toolchain; can do split/overlay/encode in one process | Harder dynamic per-output restart/control; complex command generation | Prototype candidate                 |
| GStreamer pipeline                    | Natural graph semantics, tee/queues, per-branch control                 | New runtime/tooling; higher ops complexity                            | Candidate after RFC review          |
| Rust frame pipeline + FFmpeg encoders | Maximum app-level control/health                                        | Highest implementation cost; decode/composition complexity            | Later only if FFmpeg/GStreamer fail |
| Current per-output FFmpeg             | Already works and isolates failures                                     | N decodes/encodes; no shared normalize                                | Phase 4 adapter baseline            |

Decision: wrap current per-output FFmpeg first. Choose FFmpeg graph vs GStreamer only after health/control is proven.

### Health persistence

| Option                | Pros                                                  | Cons                                   | RFC default                     |
| --------------------- | ----------------------------------------------------- | -------------------------------------- | ------------------------------- |
| Logs only             | Zero schema migration; fastest shadow-mode validation | No UI/current-state query              | Phase 1 default                 |
| D1 latest snapshot    | Simple operator UI; low write volume                  | Schema/migration + write-path pressure | Phase 3 draft                   |
| Append-only D1 events | Good audit trail; already close to session logs       | More writes; query cleanup needed      | Use existing session logs first |
| Time-series store     | Best graphs/alerts                                    | Premature ops surface                  | Not now                         |

Decision: logs first, D1 latest snapshot only when operator UI/control requires it.

### Operator control transport

| Option                   | Pros                                  | Cons                                         | RFC default                    |
| ------------------------ | ------------------------------------- | -------------------------------------------- | ------------------------------ |
| Host WS command          | Immediate; no polling                 | Requires live host socket and auth/UI wiring | Good for host-owned controls   |
| Workers/D1 command queue | Durable; works from admin/operator UI | Poll/claim complexity; delayed               | Good for admin/remote controls |
| Direct media-engine HTTP | Simple local operator action          | Auth/routing surface; bypasses Workers audit | Rehearsal only, if needed      |

Decision: define commands independent of transport. Start with logs/manual controls; choose WS vs D1 queue during Phase 3.

### GPU worker boundary

| Option                           | Pros                       | Cons                                      | RFC default                      |
| -------------------------------- | -------------------------- | ----------------------------------------- | -------------------------------- |
| In-process `server-rs` graph     | Simplest debugging; no IPC | One process owns all media CPU/GPU        | Required first proof             |
| Sidecar GPU worker same ECS task | Local IPC; easier network  | Task coupling remains                     | Candidate after in-process proof |
| Separate ECS GPU service         | Independent scale/restart  | Routing, IPC, auth, deployment complexity | Final target only                |

Decision: no worker split until in-process graph proves per-output health, restart, and burn-in.

### Billing during degradation

This is a business/ops launch-policy decision, not a blocker for Phase 1 timeline shadow mode. It must be resolved before paid V2 customer rollout or any automated degraded-minute accounting.

| Option                    | Pros                           | Cons                                        | RFC default                                 |
| ------------------------- | ------------------------------ | ------------------------------------------- | ------------------------------------------- |
| Bill full output minute   | Simple; output still delivered | Bad customer optics if TTS missing          | Avoid for captions-only                     |
| Discount degraded minutes | Fair; aligns with quality      | More billing complexity                     | Review with Simon                           |
| Free degraded minutes     | Best trust-building            | Revenue leakage; needs accurate health logs | Default recommendation for launch incidents |

Decision: RFC recommends not billing captions-only/degraded minutes during early V2 rollout unless Simon explicitly overrides.

## Phase rollback/drill plan

| Phase                  | New behavior                    | Fast rollback                                                          | Proof before next phase                    |
| ---------------------- | ------------------------------- | ---------------------------------------------------------------------- | ------------------------------------------ |
| 0 RFC/contracts        | Docs + pure tests only          | Revert docs/types; no deploy impact                                    | RFC reviewed                               |
| 1 timeline shadow      | Logs extra timing metrics       | Disable `BRIVVA_V2_TIMELINE_SHADOW`                                    | 30–60 min no drift trend, no V1 regression |
| 2 timestamped audio    | New audio clock source          | Disable `BRIVVA_V2_TIMESTAMPED_AUDIO`; old PCM remains                 | A/V drift bounded; reconnect still works   |
| 3 output controls      | Per-output commands/health      | Disable `BRIVVA_V2_OUTPUT_CONTROLS`; use session restart               | One-output restart tested                  |
| 4 render graph adapter | Current FFmpeg wrapped by graph | Disable `BRIVVA_V2_RENDER_GRAPH`; direct `RtmpManager` path            | Same outputs as V1 plus health logs        |
| 5 shared decode        | One decode/fan-out              | Disable `BRIVVA_V2_SHARED_DECODE`; return per-output encode            | 2 then 4 outputs stable                    |
| 6 GPU workers          | External heavy media worker     | Disable `BRIVVA_V2_GPU_WORKERS`; repoint `VITE_MEDIA_URL` to V1/laptop | 60–90 min proof archive                    |

Rollback drill for every phase:

1. Capture current flag values and git SHA.
2. Run V1 smoke or known-good rehearsal command.
3. Enable exactly one V2 flag.
4. Run phase proof.
5. Disable that flag.
6. Re-run V1 smoke.
7. Record pass/fail in proof archive before proceeding.

## Sequence diagrams

### Session start

```txt
Host UI
  → Workers: create/update session + stream rows
  ← Workers: session id + short-lived JWT
  → Media engine WS: connect(session_id, token, source_lang)
Media engine
  → Workers internal: fetch session bundle
  → Media engine: create LiveSession
  → Media engine: start per-output FFmpeg adapters (V1) / OutputPipelineNodes (V2)
  ← Host UI: WS accepted + preflight status
Host UI
  → Media engine: WebRTC offer(video now, audio later)
Media engine
  → Host UI: WebRTC answer
  → Logs: output.starting for each output_id
```

V2 rule: every output gets an `output_id` before media starts. Health logs must be keyed by this id from the first line.

### Live media flow

```txt
WebRTC video RTP / timestamped audio
  → MediaTimelineNode: map RTP/sample clock to media PTS
  → JitterBufferNode: reorder + readiness delay + drop stats
  → STT/Translate/TTS side path: produce transcript, translated cues, TTS PCM
  → OutputPipelineNode(output_id)
      → apply selected layers
      → mix host/TTS audio
      → encode/publish
      → emit health snapshot
```

V2 rule: STT can stay low-latency and event-driven, but anything rendered into the broadcast must carry or derive media PTS.

### One-output degradation

```txt
OutputPipelineNode(ja-youtube)
  → detects TTS timeout
  → DegradationPolicy: TTS failure = captions_only
  → output state: live → degraded
  → keep subtitle/render/video publish path alive
  → Logs: output.degraded { output_id, failure_kind, action }
Other OutputPipelineNodes
  → no state change
Session
  → stays live
```

V2 rule: degradation is per-output unless the source ingest/media clock itself fails.

### One-output restart

```txt
Health monitor
  → detects RTMP idle/crash for output_id=zh-grip
  → mark output restarting
  → stop only zh-grip encode/publish child
  → preserve source ingest + other output nodes
  → restart zh-grip from latest safe keyframe/audio point
  → mark live or failed after retry budget
```

V2 rule: restart budget exhaustion stops one output, not the whole session.

### Session stop

```txt
Host UI
  → Media engine: host:end
Media engine
  → stop all output pipelines
  → flush final metrics/health
  → close WebRTC peer
  → Workers internal: PATCH session ended
  → Logs: session.closed + output.stopped × N
```

V2 rule: normal session stop is the only path that intentionally stops all outputs together.

## Workers/D1 schema draft — no migration yet

Do not create these migrations until RFC review. Draft only.

```sql
-- Time columns are Unix milliseconds unless explicitly documented otherwise.
-- One row per configured output stream. Could extend existing streams table
-- instead of creating a new table; decide after review.
CREATE TABLE session_outputs (
  id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL,
  stream_id TEXT,
  lang TEXT NOT NULL,
  destination_platform TEXT NOT NULL,
  destination_label TEXT,
  render_layers_json TEXT NOT NULL DEFAULT '[]',
  degradation_policy_json TEXT NOT NULL DEFAULT '{}',
  created_at INTEGER NOT NULL
);

-- Latest health snapshot only. Historical events may stay in session_log_events.
CREATE TABLE session_output_health_latest (
  output_id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL,
  state TEXT NOT NULL,
  degradation TEXT NOT NULL,
  media_clock_ms INTEGER,
  video_pts_ms INTEGER,
  audio_pts_ms INTEGER,
  audio_drift_ms INTEGER,
  subtitle_lag_ms INTEGER,
  jitter_audio_depth_ms INTEGER,
  jitter_video_depth_ms INTEGER,
  drop_late_count INTEGER NOT NULL DEFAULT 0,
  drop_overflow_count INTEGER NOT NULL DEFAULT 0,
  encode_fps REAL,
  encode_speed REAL,
  encode_drop_frames INTEGER,
  tts_queue_bytes INTEGER,
  restart_count INTEGER NOT NULL DEFAULT 0,
  last_write_age_ms INTEGER,
  updated_at INTEGER NOT NULL
);

-- Operator/intake commands. Worker can append; media engine claims/applies.
CREATE TABLE session_output_commands (
  id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL,
  output_id TEXT NOT NULL,
  command TEXT NOT NULL,
  payload_json TEXT NOT NULL DEFAULT '{}',
  status TEXT NOT NULL DEFAULT 'pending',
  requested_by TEXT,
  requested_at INTEGER NOT NULL,
  applied_at INTEGER,
  error TEXT
);
```

Schema review questions:

- Extend current `streams` or introduce `session_outputs`?
- Store health latest-only in D1, or logs-only until UI needs it?
- Should commands be D1-polled, WS control messages, or both?
- How much health history belongs in `session_log_events` vs a future time-series store?

## Browser media contract draft

Preferred V2 audio path: WebRTC audio track.

```txt
Video: WebRTC H.264 RTP, 90 kHz RTP clock
Audio: WebRTC Opus RTP, 48 kHz RTP clock
Clock authority: server maps both RTP clocks to one session media timeline
```

Fallback if WebRTC audio is not ready:

```json
{
  "type": "audio:pcm",
  "format": "s16le",
  "sample_rate": 44100,
  "channels": 1,
  "sample_index_start": 123456789,
  "capture_time_ms": 456789.123,
  "payload": "binary frame follows or envelope side-channel"
}
```

Fallback rules:

- Old binary PCM frames remain accepted until V2 burn-in passes.
- New timestamped PCM is feature-gated.
- The server trusts sample index for cadence, not arrival time.
- If client sample clock jumps backward, log and fall back to arrival-time for that session.

## Burn-in proof checklist

### 2-output RFC acceptance rehearsal

- [ ] V1 smoke green before enabling any V2 flag.
- [ ] Enable timeline shadow only.
- [ ] Run 30 minutes with 2 outputs.
- [ ] Collect logs showing per-output health snapshots.
- [ ] Confirm audio/video drift bounded and not trending upward.
- [ ] Confirm no extra FFmpeg restarts versus V1 baseline.
- [ ] Disable flag and confirm V1 behavior unchanged.

### 4-output RFC acceptance rehearsal

- [ ] ECS/laptop GPU preflight green.
- [ ] Run 60–90 minutes with 4 outputs.
- [ ] Capture encode FPS/speed/drop frames per output.
- [ ] Force one TTS failure; captions continue.
- [ ] Force one RTMP failure; other outputs continue.
- [ ] Restart one output manually; session stays live.
- [ ] Archive proof logs, health snapshots, session metrics, and git SHA.

## RFC review packet

### Review scope

Reviewers should answer one question: is this the safest path from current MVP to V2 without breaking the launch path?

In scope for review:

- Current-state map accuracy.
- Target graph shape.
- Phase order and exit criteria.
- Failure-mode coverage.
- Observability payload sufficiency.
- Rollback ability per phase.
- Whether isolated domain vocabulary is useful enough to keep before implementation.

Out of scope for this review:

- Implementing flags.
- Creating D1 migrations.
- Rewriting FFmpeg args.
- Rewriting WebRTC ingest.
- Changing ECS/Fargate/GPU deployment.
- UI work for output controls.

### Files in the RFC packet

V2-owned:

- `docs/v2-media-engine.md`
- `server-rs/src/features/broadcast/domain/media_timeline.rs`
- `server-rs/src/features/broadcast/domain/render_graph.rs`
- `server-rs/src/features/broadcast/domain/mod.rs` — module exports only

Not V2-owned / exclude from this review even if present in working tree:

- `frontend/.pi-lens/turn-state.json`
- `frontend/playwright.real-stack.config.ts`
- `workers/src/orchestration/app.ts`
- `scripts/test-local-nvenc-youtube.sh`
- `server-rs/.pi-lens/turn-state.json`

### Reviewer questions

1. Does V2 really need WebRTC audio first, or is timestamped PCM enough for the next paid milestone?
2. Should health persistence start logs-only, or is D1 latest-snapshot required before any operator UI?
3. Is per-output command queue over D1 worth the durability, or should first controls ride the host WS?
4. Do we accept current per-output FFmpeg as the Phase 4 adapter baseline even though it keeps N encodes?
5. Which shared-video prototype should happen first after RFC review: FFmpeg complex graph or GStreamer?
6. What customer-facing billing rule applies when an output is captions-only/degraded?

### Merge/review gate

Before implementation starts:

- [ ] RFC reviewed by Aziz.
- [ ] Current-state claims verified against code.
- [ ] Old WebRTC migration doc conflicts resolved or explicitly superseded.
- [ ] Phase 1 flags named and accepted.
- [ ] Burn-in proof checklist accepted.
- [ ] Decision recorded for logs-only vs D1 latest health in Phase 1/3.
- [ ] Decision recorded for WebRTC audio vs timestamped PCM first.

## Rollback runbook details — draft, do not execute until flags exist

### Universal rollback invariant

Every V2 phase must be disabled by configuration/env first. Code rollback is last resort.

```txt
Detect bad V2 behavior
  ↓
Disable newest V2 flag
  ↓
Restart affected output only if controls exist
  ↓
Restart session only if output restart fails
  ↓
Repoint VITE_MEDIA_URL to V1/laptop fallback if engine is unstable
  ↓
Rollback ECS task/image only if V1 path is also broken
```

### Phase rollback details

| Phase                | Failure symptom                                      | First rollback                                     | Validation after rollback                         |
| -------------------- | ---------------------------------------------------- | -------------------------------------------------- | ------------------------------------------------- |
| Timeline shadow      | Logs noisy/CPU overhead/drift math wrong             | Disable `BRIVVA_V2_TIMELINE_SHADOW`                | V1 smoke green; no output behavior changed        |
| Timestamped audio    | STT starves, audio drift jumps, client clock invalid | Disable `BRIVVA_V2_TIMESTAMPED_AUDIO`; use old PCM | Host audio reaches STT; translated output resumes |
| Output controls      | Wrong output restarts/stops                          | Disable `BRIVVA_V2_OUTPUT_CONTROLS`                | Manual session restart still works                |
| Render graph adapter | Output missing layer/audio/video                     | Disable `BRIVVA_V2_RENDER_GRAPH`                   | Current `RtmpManager` path publishes all outputs  |
| Shared decode        | FPS drops, frames out of sync                        | Disable `BRIVVA_V2_SHARED_DECODE`                  | Per-output FFmpeg path stable again               |
| GPU workers          | Worker unreachable/crashes                           | Disable `BRIVVA_V2_GPU_WORKERS`; repoint media URL | Laptop/V1 media engine smoke green                |

### Required rollback evidence

For every phase rehearsal, collect:

- Git SHA.
- Enabled V2 flags.
- Session id.
- Output ids.
- Health snapshots before failure.
- Exact rollback action.
- Logs proving output/session recovered.
- D1 session state after recovery.

### 2am operator rules

- If source ingest is broken, stop debugging output nodes. Restart/repoint the whole media engine.
- If one output is broken, do not restart the session until one-output restart/stop has failed.
- If health logs cannot identify the bad output within 30s, V2 observability is not launch-ready.
- If disabling a V2 flag requires a code deploy, that phase is not ready.

## Implementation freeze checklist

- [x] Live-path WebRTC edit reverted.
- [x] No FFmpeg pipeline edits in this RFC packet.
- [x] No Workers runtime or D1 migration in this RFC packet.
- [x] No deployment/Terraform changes in this RFC packet.
- [x] Remaining V2 code is pure domain/test scaffolding only.
- [ ] RFC reviewed before any production-code follow-up task is created.

## Final RFC review summary

### Recommendation

Approve the RFC direction, but keep implementation frozen until the review checklist below is explicitly accepted. The safest first implementation after approval is **timeline shadow mode only**: compute media PTS/jitter/drift in logs without driving output.

### Why this path is low-risk

- It does not replace the current host WS/audio or current WebRTC video path first.
- It does not introduce a worker boundary before in-process observability exists.
- It wraps current per-output FFmpeg isolation before trying shared decode.
- It keeps old PCM fallback and current session restart fallback.
- Every phase has an env/config rollback concept before code rollout.

### Biggest unresolved decisions

1. **Audio first path:** WebRTC Opus vs timestamped PCM bridge.
2. **Health storage:** logs-only for Phase 1 vs D1 latest snapshot before operator UI.
3. **Control transport:** host WS vs D1 command queue.
4. **Shared video prototype:** FFmpeg complex graph vs GStreamer.
5. **Billing:** whether degraded/captions-only minutes are free during early V2 rollout.

### First post-review task packet — draft only

Do not create this task until RFC review accepts it.

```txt
Task: V2 timeline shadow mode
Scope:
- Add feature flag BRIVVA_V2_TIMELINE_SHADOW.
- Compute video media PTS/jitter from existing video RTP.
- Emit approximate audio arrival/queue timing only, clearly labeled non-authoritative until Phase 2 timestamped audio.
- Emit logs only; do not drive FFmpeg drains or alter output.
- Add tests for pure timestamp math and shadow metric formatting.
Exit:
- V1 smoke green before/after flag.
- 30 min 2-output rehearsal with no output behavior change.
- Logs enough to identify drift/jitter per output/session.
Rollback:
- Disable BRIVVA_V2_TIMELINE_SHADOW.
```

### Second post-review task packet — draft only

```txt
Task: V2 timestamped audio decision/prototype
Scope:
- Decide WebRTC Opus vs timestamped PCM first.
- Implement behind BRIVVA_V2_TIMESTAMPED_AUDIO only after timeline shadow proof.
- Keep old raw PCM accepted.
Exit:
- Audio/video drift bounded in rehearsal.
- STT path survives reconnect/fallback.
Rollback:
- Disable BRIVVA_V2_TIMESTAMPED_AUDIO.
```

## RFC completeness scorecard

| Area                       | Status          | Remaining review action                                |
| -------------------------- | --------------- | ------------------------------------------------------ |
| Current-state map          | Draft complete  | Verify file paths and claims against current code      |
| Target architecture        | Draft complete  | Accept in-process-first graph and delayed worker split |
| Migration phases           | Draft complete  | Accept phase order and one-flag-at-a-time rule         |
| Failure modes              | Draft complete  | Confirm no launch-critical failure class missing       |
| Observability              | Draft complete  | Confirm payload identifies bad output within 30s       |
| Rollback                   | Draft complete  | Confirm each phase can roll back by flag/config first  |
| Older WebRTC plan conflict | Resolved in RFC | Accept this RFC as V2 sequencing source of truth       |
| Production code            | Frozen          | Do not proceed until review checklist accepted         |

## Review decision record template

Fill this during review before creating implementation tasks.

```txt
Review date:
Reviewer:
Decision: approve / approve with changes / reject
Required RFC edits before implementation:
Accepted first implementation task:
Audio path decision: WebRTC Opus / timestamped PCM / defer
Health persistence decision: logs-only / D1 latest / other
Control transport decision: host WS / D1 command queue / defer
Shared video prototype decision: FFmpeg graph / GStreamer / defer
Billing decision for degraded minutes:
Go/no-go notes:
```

## Post-review implementation order — draft only

1. Timeline shadow mode.
2. Timestamped audio decision/prototype.
3. Per-output health snapshot logs.
4. Per-output controls with no UI or D1 migration unless review approves.
5. Render graph adapter around current FFmpeg streams.
6. Shared decode prototype.
7. GPU worker split.

Stop after each item for proof + rollback validation. Do not batch phases.

## RFC review checklist

- [ ] Current-state map matches code reality.
- [ ] `docs/webrtc-migration-plan.md` is accepted as superseded by this RFC for V2 sequencing.
- [ ] Target architecture has no hidden worker boundary before in-process proof.
- [ ] Every phase has exit criteria and rollback.
- [ ] Failure matrix covers TTS, voice, subtitles, overlays, RTMP, encoder, GPU, clock drift, jitter overflow.
- [ ] Observability payload is enough to identify failing output in under 30s.
- [ ] D1 schema draft does not force premature migration and all timestamp fields are Unix milliseconds.
- [ ] Browser media contract keeps old PCM fallback.
- [ ] First implementation task is limited to timeline shadow mode.
- [ ] No production-code implementation starts before this checklist is reviewed.
