#!/usr/bin/env bash
#
# test-all.sh — run every test suite in the repo in parallel, stream each
# crate's output to its own log file, and print a summary at the end.
#
# Suites:
#   - server-rs   (cargo test)
#   - workers     (bun test)
#   - frontend    (bun test)
#
# Usage:
#   ./scripts/test-all.sh            # parallel (default)
#   ./scripts/test-all.sh --serial   # sequential — easier to read logs
#
# Exit codes:
#   0 — all suites green
#   N — number of failing suites
#
# This does NOT boot any services; it runs each workspace's unit/integration
# suite as it would in CI. For an end-to-end smoke against a running stack,
# use ./scripts/dev-stack.sh (which boots services + runs smoke-test.sh).
#
# Compat: bash 3.2+ (macOS system bash). No associative arrays.

set -u

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
LOG_DIR="${REPO_ROOT}/.test-logs"
mkdir -p "$LOG_DIR"

RED=$'\033[0;31m'
GREEN=$'\033[0;32m'
YELLOW=$'\033[0;33m'
DIM=$'\033[2m'
NC=$'\033[0m'

SERIAL=0
[ "${1:-}" = "--serial" ] && SERIAL=1

# Parallel arrays — indexed by suite number. Keep them in lockstep.
NAMES=(server-rs     workers  frontend)
DIRS=(server-rs      workers  frontend)
CMDS=("cargo test --quiet"  "bun run test"  "bun run test")

PIDS=()  # filled when parallel
RCS=()   # filled after wait()

run_one() {
  # $1 = index
  local i=$1
  local name="${NAMES[$i]}"
  local dir="${DIRS[$i]}"
  local cmd="${CMDS[$i]}"
  local log="${LOG_DIR}/${name}.log"
  echo "${DIM}[${name}] starting → ${log}${NC}"
  ( cd "${REPO_ROOT}/${dir}" && eval "$cmd" ) >"$log" 2>&1
  local rc=$?
  if [ $rc -eq 0 ]; then
    echo "${GREEN}✓ ${name}${NC}"
  else
    echo "${RED}✗ ${name} (exit ${rc}) — see ${log}${NC}"
  fi
  return $rc
}

echo "Brivva test-all — logs in ${LOG_DIR}"
echo ""

n=${#NAMES[@]}
i=0
while [ $i -lt $n ]; do
  if [ $SERIAL -eq 1 ]; then
    run_one $i
    RCS[$i]=$?
  else
    run_one $i &
    PIDS[$i]=$!
  fi
  i=$((i + 1))
done

if [ $SERIAL -eq 0 ]; then
  i=0
  while [ $i -lt $n ]; do
    wait "${PIDS[$i]}"
    RCS[$i]=$?
    i=$((i + 1))
  done
fi

echo ""
echo "── summary ───────────────────────────────"
fails=0
i=0
while [ $i -lt $n ]; do
  name="${NAMES[$i]}"
  rc="${RCS[$i]}"
  if [ "$rc" = "0" ]; then
    echo "${GREEN}✓ ${name}${NC}"
  else
    echo "${RED}✗ ${name} (exit ${rc})${NC}  ${DIM}tail -n 40 ${LOG_DIR}/${name}.log${NC}"
    fails=$((fails + 1))
  fi
  i=$((i + 1))
done

if [ $fails -eq 0 ]; then
  echo ""
  echo "${GREEN}all green${NC}"
  exit 0
else
  echo ""
  echo "${YELLOW}${fails} suite(s) failed${NC}"
  exit $fails
fi
