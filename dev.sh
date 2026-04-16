#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")" && pwd)"
PIDS=()

kill_port_if_busy() {
    local port="$1"
    local pids
    pids="$(lsof -tiTCP:"$port" -sTCP:LISTEN 2>/dev/null || true)"
    if [[ -n "$pids" ]]; then
        echo "[dev] freeing port :$port ($pids)"
        kill $pids 2>/dev/null || true
        sleep 1
    fi
}

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

kill_port_if_busy 3000
kill_port_if_busy 5173
kill_port_if_busy 1935
kill_port_if_busy 8888

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
echo "    http://localhost:8888/live/test/index.m3u8"
echo ""
echo "  Note:"
echo "    HLS manifest appears only after publisher is sending media."
echo "    Many browsers need an HLS-capable player; VLC works directly."
echo "═══════════════════════════════════════════════════════"
echo ""
echo "Press Ctrl+C to stop all services"

wait
