#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

echo "[phase6b] flag off: no local worker logs, current route selected"
cd "$ROOT/server-rs"
unset BRIVVA_V2_GPU_WORKERS || true
cargo test -p server-rs flag_off_keeps_current_route_and_emits_no_local_worker_logs --lib

echo "[phase6b] flag on: one test output uses same-host local worker proof route"
BRIVVA_V2_GPU_WORKERS=1 cargo test -p server-rs worker_start_ready_stop_lifecycle_logs_local_only --lib

echo "[phase6b] worker failure: stale revoke logs and current FFmpeg fallback route selected"
BRIVVA_V2_GPU_WORKERS=1 cargo test -p server-rs stale_worker_revoke_falls_back_to_current_route --lib

echo "[phase6b] lease fencing + customer/live destination rejection"
BRIVVA_V2_GPU_WORKERS=1 cargo test -p server-rs health_is_fenced_by_lease_worker_and_generation --lib
BRIVVA_V2_GPU_WORKERS=1 cargo test -p server-rs route_selection_rejects_customer_or_real_destination_by_default --lib

echo "[phase6b] rollback drill: disabling flag returns to current route within 2 minutes"
START_MS=$(date +%s%3N)
unset BRIVVA_V2_GPU_WORKERS || true
cargo test -p server-rs flag_off_keeps_current_route_and_emits_no_local_worker_logs --lib
END_MS=$(date +%s%3N)
ELAPSED_MS=$((END_MS - START_MS))
if [ "$ELAPSED_MS" -gt 120000 ]; then
	echo "rollback drill exceeded 2 minutes: ${ELAPSED_MS}ms" >&2
	exit 1
fi

echo "[phase6b] static safety: local proof has no network/public/cloud/deploy/process-spawn primitives"
if grep -R "TcpListener\|TcpStream\|UdpSocket\|axum::Router\|terraform\|docker\|Command::new\|tokio::process\|std::process" -n src/features/broadcast/domain/gpu_worker_local.rs; then
	echo "local worker proof references forbidden network/public/cloud/deploy/process-spawn primitive" >&2
	exit 1
fi

cd "$ROOT"
echo "[phase6b] production FFmpeg topology guard: no args/drain/mixer/pacing/publish files modified"
if git diff --name-only -- \
	server-rs/src/features/broadcast/data/ffmpeg/args.rs \
	server-rs/src/features/broadcast/data/ffmpeg/drain.rs \
	server-rs/src/features/broadcast/data/ffmpeg/mixer.rs \
	server-rs/src/features/broadcast/data/ffmpeg/mod.rs \
	server-rs/src/features/broadcast/data/session_ws/rtmp.rs | grep .; then
	echo "forbidden production FFmpeg route/touchpoint changed" >&2
	exit 1
fi

echo "[phase6b] proof passed: local same-host test-output route only; flag-off rollback=${ELAPSED_MS}ms; current production FFmpeg path unchanged"
