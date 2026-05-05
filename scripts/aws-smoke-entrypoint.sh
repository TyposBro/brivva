#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PROFILE="${AWS_SMOKE_PROFILE:-fake-sink}"
RUN_ID="${AWS_SMOKE_RUN_ID:-$(date -u +%Y%m%d-%H%M)-${PROFILE}}"
RUN_DIR="${AWS_SMOKE_RUN_DIR:-$ROOT/tmp/aws-soak-runs/$RUN_ID}"
MP4="${MP4_FANOUT_SMOKE_MP4:-/mnt/fixtures/text.h264.mp4}"
SOURCE_LANG="${MP4_FANOUT_SMOKE_SOURCE_LANG:-ko}"
DURATION="${MP4_FANOUT_SMOKE_DURATION:-600}"
AWS_REGION="${AWS_REGION:-us-east-1}"

mkdir -p "$RUN_DIR"

cat >"$RUN_DIR/manifest.md" <<EOF
# AWS Smoke Manifest

Run: $RUN_ID
Profile: $PROFILE
Region: $AWS_REGION
Created UTC: $(date -u +%Y-%m-%dT%H:%M:%SZ)
Commit: $(git -C "$ROOT" rev-parse HEAD 2>/dev/null || echo unknown)
Fixture: $MP4
Source language: $SOURCE_LANG
Duration seconds: $DURATION

## Safety

This entrypoint does not scale AWS, deploy, mutate secrets, or contact real RTMP platforms.
It prepares a fake/local-sink rehearsal command for an already-running AWS task/container path.
Fake sink proves AWS image/GPU/media capability only; it is not production platform proof.
EOF

cat >"$RUN_DIR/aws-fake-sink-smoke.env" <<EOF
AWS_REGION=$AWS_REGION
MP4_FANOUT_SMOKE_MP4=$MP4
MP4_FANOUT_SMOKE_SOURCE_LANG=$SOURCE_LANG
MP4_FANOUT_SMOKE_DURATION=$DURATION
MP4_FANOUT_SMOKE_ENCODER=nvenc
BRIVVA_VIDEO_ENCODER=nvenc
BRIVVA_VIDEO_MAX_WIDTH=1920
BRIVVA_VIDEO_MAX_HEIGHT=1080
BRIVVA_VIDEO_MAX_FPS=30
BRIVVA_RTMP_OUTPUT_LAYOUT=portrait
RUST_STRESS_MAX_TTS_OVERFLOWS=0
RUST_STRESS_MAX_HARD_RECOVERY=0
RUST_STRESS_MAX_SLOW_ENCODE_TICKS=20
# Intentionally omit STREAM_URL_* and STREAM_KEY_* real platform secrets.
# Use the server-rs/local fake sink path only.
EOF

cat >"$RUN_DIR/grep-patterns.txt" <<'EOF'
ffmpeg rtmp process crashed|publisher restart|ffmpeg restart|restart attempts exhausted
encode below realtime|speed below realtime|drop_frames=|speed=0\.[0-8]
video_stale_chunks_dropped|host_audio_stale_chunks_dropped|ready_host_bytes_dropped|mp4 smoke dropping (video|audio) chunk
tts segment queue overflow
final_policy="hard_recovery"|hard_recovery
EOF

cat >"$RUN_DIR/grep-patterns.md" <<'EOF'
# Pass/fail grep patterns for smoke.log / CloudWatch export

- FFmpeg restart/crash: `ffmpeg rtmp process crashed|publisher restart|ffmpeg restart|restart attempts exhausted` (PASS: 0 lines)
- Sustained below realtime: `encode below realtime|speed below realtime|drop_frames=|speed=0\.[0-8]` (PASS: not sustained; runner allowance `RUST_STRESS_MAX_SLOW_ENCODE_TICKS`)
- Media drops: `video_stale_chunks_dropped|host_audio_stale_chunks_dropped|ready_host_bytes_dropped|mp4 smoke dropping (video|audio) chunk` (PASS: 0 lines)
- TTS overflow: `tts segment queue overflow` (PASS: 0 lines unless explicitly allowed)
- Hard recovery: `final_policy="hard_recovery"|hard_recovery` (PASS: 0 lines unless explicitly allowed)
EOF

cat <<EOF
Prepared AWS fake/local-sink smoke folder:
  $RUN_DIR

Safe command to run INSIDE an already-started AWS rehearsal container/task shell (no real platform keys):

  cd /app && \
  env \
    MP4_FANOUT_SMOKE_MP4='$MP4' \
    MP4_FANOUT_SMOKE_SOURCE_LANG='$SOURCE_LANG' \
    MP4_FANOUT_SMOKE_DURATION='$DURATION' \
    MP4_FANOUT_SMOKE_ENCODER=nvenc \
    BRIVVA_VIDEO_ENCODER=nvenc \
    BRIVVA_VIDEO_MAX_WIDTH=1920 \
    BRIVVA_VIDEO_MAX_HEIGHT=1080 \
    BRIVVA_VIDEO_MAX_FPS=30 \
    RUST_STRESS_MAX_TTS_OVERFLOWS=0 \
    RUST_STRESS_MAX_HARD_RECOVERY=0 \
    cargo test -p server-rs --test mp4_fanout_smoke -- --ignored --nocapture 2>&1 | tee smoke.log

If running from repo checkout on the AWS host/container image:

  ./scripts/run-rust-stress-tests.sh --only single_1080p30 --mp4 '$MP4' --source '$SOURCE_LANG' --duration '$DURATION'

After CloudWatch/local log export, check:

  rg -n -f '$RUN_DIR/grep-patterns.txt' <smoke.log-or-cloudwatch-log>

This script only prepared files and printed commands. It did not run AWS scale-up/deploy/stream actions.
EOF
