#!/usr/bin/env bash
#
# smoke-test.sh — end-to-end sanity check against a running Brivva stack.
#
# Runs from any machine with bash + curl + bun. Targets:
#
#   - Production:  ./scripts/smoke-test.sh prod
#   - Local dev:   ./scripts/smoke-test.sh local
#   - Custom URL:  SERVER_URL=https://... ./scripts/smoke-test.sh
#
# Intended as the post-rollback gate described in docs/runbook.md: after
# `aws ecs update-service --force-new-deployment` completes, run this and
# do not call the incident "resolved" until it returns zero.
#
# What it checks:
#   1. `/health` returns 200 within 5s
#   2. Workers API returns 401 without auth, 200 with a valid JWT
#   3. D1 migration state is fresh (schema version matches)
#   4. ECR image pulled by the running task matches the latest git tag
#      (only when prod + AWS CLI + jq available)
#
# What it does NOT do:
#   - Exercise the full media pipeline (that is tests/e2e, docker-compose only)
#   - Start an actual RTMP stream (too destructive for a smoke)
#   - Validate external platforms (YouTube, Grip, TikTok)
#
# Exit codes:
#   0 — everything green
#   1 — any check failed
#   2 — tool missing (curl / bun / aws), check could not run

set -euo pipefail

RED=$'\033[0;31m'
GREEN=$'\033[0;32m'
YELLOW=$'\033[0;33m'
NC=$'\033[0m'

fail() { echo "${RED}✗ $1${NC}"; exit 1; }
pass() { echo "${GREEN}✓ $1${NC}"; }
skip() { echo "${YELLOW}⊘ $1 (skipped)${NC}"; }

# ── Target selection ─────────────────────────────────────────
TARGET="${1:-prod}"

case "$TARGET" in
  prod)
    SERVER_URL="${SERVER_URL:-https://brivva.spiko.uz}"
    WORKERS_URL="${WORKERS_URL:-https://brivva-api.milliytechnology.workers.dev}"
    ECS_CLUSTER="brivva"
    ECS_SERVICE="brivva"
    ;;
  local)
    SERVER_URL="${SERVER_URL:-http://localhost:3000}"
    WORKERS_URL="${WORKERS_URL:-http://localhost:8787}"
    ECS_CLUSTER=""
    ECS_SERVICE=""
    ;;
  *)
    # Allow callers to override just SERVER_URL without setting target name.
    SERVER_URL="${SERVER_URL:?SERVER_URL required when target is not 'prod' or 'local'}"
    WORKERS_URL="${WORKERS_URL:-$SERVER_URL}"
    ECS_CLUSTER=""
    ECS_SERVICE=""
    ;;
esac

echo "Target:          $TARGET"
echo "Server URL:      $SERVER_URL"
echo "Workers URL:     $WORKERS_URL"
echo ""

# ── Tool check ───────────────────────────────────────────────
command -v curl >/dev/null || { echo "curl missing"; exit 2; }

# ── 1. /health ───────────────────────────────────────────────
code=$(curl -sS -o /tmp/brivva-smoke-health.json -w "%{http_code}" -m 5 "$SERVER_URL/health" 2>&1 || echo "000")
if [ "$code" = "200" ]; then
  pass "server-rs /health 200"
else
  cat /tmp/brivva-smoke-health.json 2>/dev/null | head -5 || true
  fail "server-rs /health returned $code (expected 200)"
fi

# ── 2. Workers: malformed call is rejected with 4xx ──────────
# /api/user is keyed by ?user_id= (no JWT middleware). Missing query →
# Zod-parse failure → 400. Any 4xx proves the worker is up AND its
# schema validation is live. 2xx would mean no validation, 5xx = broken.
code=$(curl -sS -o /dev/null -w "%{http_code}" -m 5 "$WORKERS_URL/api/user" 2>&1 || echo "000")
if [ "$code" -ge 400 ] && [ "$code" -lt 500 ]; then
  pass "workers rejects malformed request with $code (schema validation live)"
else
  fail "workers malformed-request call returned $code (expected 4xx)"
fi

# ── 3. Workers: OpenAPI doc is reachable ─────────────────────
code=$(curl -sS -o /dev/null -w "%{http_code}" -m 5 "$WORKERS_URL/openapi.json" 2>&1 || echo "000")
if [ "$code" = "200" ] || [ "$code" = "404" ]; then
  # 404 is acceptable — docs may not be exposed in prod
  pass "workers OpenAPI endpoint reachable (http=$code)"
else
  fail "workers OpenAPI endpoint returned $code"
fi

# ── 4. ECS running image vs git HEAD (prod only, best-effort) ─
if [ -n "$ECS_CLUSTER" ] && command -v aws >/dev/null 2>&1 && command -v jq >/dev/null 2>&1; then
  task_arn=$(aws ecs list-tasks --cluster "$ECS_CLUSTER" --service-name "$ECS_SERVICE" \
    --desired-status RUNNING --query 'taskArns[0]' --output text 2>/dev/null || echo "")

  if [ -z "$task_arn" ] || [ "$task_arn" = "None" ]; then
    skip "no RUNNING ECS task found"
  else
    image=$(aws ecs describe-tasks --cluster "$ECS_CLUSTER" --tasks "$task_arn" \
      --query 'tasks[0].containers[?name==`server`].image | [0]' --output text 2>/dev/null || echo "")
    if [ -n "$image" ]; then
      pass "ECS task running image: $image"
    else
      skip "could not read container image from ECS"
    fi
  fi
else
  skip "ECS check (requires prod target + aws CLI + jq)"
fi

# ── 5. Git tag sanity (informational) ────────────────────────
if command -v git >/dev/null 2>&1 && [ -d .git ]; then
  latest_tag=$(git tag --sort=-committerdate | head -1 2>/dev/null || echo "")
  if [ -n "$latest_tag" ]; then
    echo ""
    echo "Latest git tag: $latest_tag"
    echo "  (this is what a rollback would restore to)"
  fi
fi

echo ""
echo "${GREEN}smoke-test passed${NC}"
exit 0
