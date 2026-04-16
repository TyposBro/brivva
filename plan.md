# Brivva Rebuild Plan

**Date:** April 16, 2026  
**Status:** Reset. Assume empty repo. Rebuild streaming core from scratch.

## Rule Zero

Source stream must work perfectly before translation exists.

Perfect means:
- no YouTube buffering
- no A/V drift
- no periodic pauses
- stable long-run stream
- predictable delay

## Non-Goals For V1

Do not build first:
- host underlay at 20%
- subtitles
- voice cloning
- multi-language fanout
- restart logic
- SaaS infra
- billing
- dashboard polish

## Phase 1: Source-Only Streaming Core

### Goal

One stable RTMP output:
- delayed source video
- delayed source audio
- both on same timeline

### Deliverables

1. Browser capture contract
- audio frames with timestamps
- video chunks with timestamps

2. Server ingest layer
- websocket receiver
- validation
- ring buffers for audio and video

3. Timeline scheduler
- one monotonic server clock
- play_at = capture_ts + configured_delay
- same rule for audio and video

4. RTMP mux output
- FFmpeg process
- source-only output
- minimal configuration

5. Verification
- 30+ minute run
- no drift
- no buffering
- no disconnect

## Phase 2: Translation As Separate Consumer

### Goal

Keep source path untouched. Add translated stream on top.

### Deliverables

1. STT endpoint pipeline
- transcript
- translation text

2. TTS pipeline
- translated audio chunks
- utterance timestamps

3. Translated stream output
- same delayed video timeline as source
- translated audio inserted into matching timeline slots
- no host underlay in first version

4. Verification
- continuity under load
- acceptable translation latency
- source stream still stable

## Phase 3: Optional Features

Only after Phase 1 and 2 proven stable.

Possible additions:
- host underlay
- subtitles
- multi-target outputs
- restart/recovery logic
- health UI
- per-language voice config

## Architecture Principles

1. One timeline.
- audio and video use same scheduling rule

2. No hidden timing hacks.
- no startup realign
- no ad hoc queue trimming
- no separate “audio delay model” vs “video delay model”

3. Backpressure visible.
- queue depth measurable
- late frames measurable
- drop policy explicit

4. Source path protected.
- translation failure must not break source stream

5. Add complexity only after proof.
- stable baseline first

## Build Order

1. Define wire format for timestamped audio/video messages
2. Implement browser sender
3. Implement server buffers
4. Implement scheduler
5. Implement source RTMP output
6. Run endurance tests
7. Add STT/TTS
8. Add translated RTMP output
9. Add optional extras

## Success Checklist

### Phase 1
- [ ] Source stream starts reliably
- [ ] Audio and video aligned
- [ ] Delay stays stable over time
- [ ] No YouTube buffering
- [ ] No frequent disconnects
- [ ] CPU stays within acceptable range

### Phase 2
- [ ] Translation does not break source stream
- [ ] Translated stream remains continuous
- [ ] TTS late/missing cases handled cleanly
- [ ] Per-language outputs remain isolated

## Working Assumption

Legacy code not trusted. Reuse ideas only if they survive clean redesign.
