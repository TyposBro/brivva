#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")" && pwd)"
PIDS=()

cleanup() {
    echo ""
    echo "[dev] shutting down..."
    for pid in "${PIDS[@]}"; do
        kill "$pid" 2>/dev/null || true
    done
    wait 2>/dev/null
    echo "[dev] done"
}
trap cleanup EXIT INT TERM

# ── 1. Local RTMP server (mediamtx) ──────────────────────────────
echo "[dev] starting mediamtx (RTMP on :1935, HLS on :8888)..."
mediamtx &
PIDS+=($!)
sleep 1

# ── 2. Rust backend ─────────────────────────────────────────────
echo "[dev] building + starting backend on :3000..."
(cd "$ROOT/server" && cargo run 2>&1 | sed 's/^/[server] /') &
PIDS+=($!)
sleep 2

# ── 3. Frontend dev server ──────────────────────────────────────
echo "[dev] starting vite on :5173..."
(cd "$ROOT/frontend" && npx vite --host 2>&1 | sed 's/^/[vite] /') &
PIDS+=($!)
sleep 1

# ── Ready ────────────────────────────────────────────────────────
echo ""
echo "═══════════════════════════════════════════════════════"
echo "  Frontend:  http://localhost:5173"
echo "  Backend:   http://localhost:3000"
echo ""
echo "  RTMP URL (paste into frontend):"
echo "    rtmp://localhost:1935/live/test"
echo ""
echo "  Watch stream (paste in browser or VLC):"
echo "    http://localhost:8888/live/test"
echo "═══════════════════════════════════════════════════════"
echo ""
echo "Press Ctrl+C to stop all services"

wait
