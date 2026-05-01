#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT/server-rs"

echo "[phase6a] flag off: focused contract tests still pass and shadow proof API emits no logs"
unset BRIVVA_V2_GPU_WORKERS || true
cargo test gpu_worker_shadow --lib
cargo test from_env_applies_soniox_and_elevenlabs_defaults_when_unset --lib

echo "[phase6a] flag on: focused contract tests pass with BRIVVA_V2_GPU_WORKERS enabled"
BRIVVA_V2_GPU_WORKERS=1 cargo test gpu_worker_shadow --lib
BRIVVA_V2_GPU_WORKERS=1 cargo test from_env_populates_secrets_and_kill_switches_from_environment --lib

echo "[phase6a] static safety check: no process/network/deploy wiring in shadow contract"
if grep -R "Command::new\|tokio::process\|std::process\|TcpStream\|UdpSocket\|terraform\|docker" -n src/features/broadcast/domain/gpu_worker_shadow.rs; then
	echo "shadow contract references forbidden process/network/deploy primitives" >&2
	exit 1
fi

echo "[phase6a] proof passed: local contracts/tests only; no live media route changed"
