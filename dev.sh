#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")" && pwd)"
PIDS=()
IS_CLEANING_UP=0

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

stop_services() {
    echo ""
    echo "[dev] shutting down..."
    for pid in "${PIDS[@]}"; do
        kill -TERM -- "-$pid" 2>/dev/null || true
    done
    wait 2>/dev/null
    PIDS=()

    echo "[dev] cleaning stale brivva ffmpeg/fifo..."
    pkill -f "/tmp/brivva_audio_" 2>/dev/null || true
    rm -f /tmp/brivva_audio_* 2>/dev/null || true
}

cleanup() {
    if [[ "$IS_CLEANING_UP" -eq 1 ]]; then
        return
    fi
    IS_CLEANING_UP=1
    stop_services
    echo "[dev] done"
}
trap 'cleanup; exit 0' INT TERM
trap cleanup EXIT

wait_for_port() {
    local port="$1"
    local label="$2"
    local attempts="${3:-50}"

    for ((i = 0; i < attempts; i++)); do
        if lsof -tiTCP:"$port" -sTCP:LISTEN >/dev/null 2>&1; then
            return 0
        fi
        sleep 0.2
    done

    echo "[dev] $label failed to listen on :$port"
    return 1
}

start_service() {
    local name="$1"
    local workdir="$2"
    local command="$3"

    (
        cd "$workdir"
        exec python3 -c 'import os, sys; os.setsid(); os.execvp(sys.argv[1], sys.argv[1:])' \
            bash -lc "$command" \
            > >(sed "s/^/[$name] /") \
            2> >(sed "s/^/[$name] /" >&2)
    ) &

    PIDS+=($!)
}

start_all() {
    kill_port_if_busy 3000
    kill_port_if_busy 5173
    kill_port_if_busy 1935
    kill_port_if_busy 8888

    echo "[dev] cleaning stale brivva ffmpeg/fifo..."
    pkill -f "/tmp/brivva_audio_" 2>/dev/null || true
    rm -f /tmp/brivva_audio_* 2>/dev/null || true

    # ── 1. Local RTMP server (mediamtx) ──────────────────────────────
    echo "[dev] starting mediamtx (RTMP on :1935, HLS on :8888)..."
    start_service "mediamtx" "$ROOT" "exec mediamtx"
    wait_for_port 1935 "mediamtx RTMP"
    wait_for_port 8888 "mediamtx HLS"

    # ── 2. Rust backend ─────────────────────────────────────────────
    echo "[dev] building + starting backend on :3000..."
    start_service "server" "$ROOT/server" "exec cargo run"
    wait_for_port 3000 "backend"

    # ── 3. Frontend dev server ──────────────────────────────────────
    echo "[dev] starting vite on :5173..."
    start_service "vite" "$ROOT/frontend" "exec npx vite --host"
    wait_for_port 5173 "vite"

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
    echo "Press R to restart, Ctrl+C to stop"
}

start_all

while true; do
    if read -rsn1 -t1 key 2>/dev/null; then
        if [[ "$key" == "r" || "$key" == "R" ]]; then
            stop_services
            clear
            echo "[dev] restarting..."
            start_all
        fi
    fi
done
