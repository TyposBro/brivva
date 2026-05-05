#!/usr/bin/env bash
# Run Ralph review swarm + judge after RS-007 builder agents finish.
# Collects builder worktree diffs, spawns read-only reviewers, then runs a judge.

set -euo pipefail

ROOT="$(git rev-parse --show-toplevel)"
BASE_DIR="${BASE_DIR:-$(dirname "$ROOT")/brivva-rs007-agents}"
RUN_DIR="$ROOT/tmp/rs007-review-swarm/$(date -u +%Y%m%d-%H%M%S)"
LATEST_LINK="$ROOT/tmp/rs007-review-swarm/latest"
SESSION="${SESSION:-rs007-review}"
PI_BIN="${PI_BIN:-pi}"
MODEL_ARGS="${MODEL_ARGS:-}"
FORCE="${FORCE:-0}"
START_PI="${START_PI:-1}"

builders=(
  "infra-auditor"
  "runtime-self-check"
  "aws-smoke-entrypoint"
  "secrets-isolation"
  "log-export-verdict"
  "billing-drill"
)

ralphs=(
  "security-secret-critic"
  "aws-network-critic"
  "cost-rollback-critic"
  "observability-critic"
  "billing-critic"
  "operator-launch-critic"
)

cd "$ROOT"

if ! command -v "$PI_BIN" >/dev/null 2>&1; then
  echo "pi not found. Set PI_BIN=/path/to/pi" >&2
  exit 1
fi

if ! command -v tmux >/dev/null 2>&1 && [[ "$START_PI" == "1" ]]; then
  echo "tmux not found. Install tmux or run START_PI=0 $0" >&2
  exit 1
fi

mkdir -p "$RUN_DIR/builder-diffs" "$RUN_DIR/reviews" "$RUN_DIR/tasks"
rm -f "$LATEST_LINK"
ln -s "$RUN_DIR" "$LATEST_LINK"

# Capture builder diffs + status from worktrees.
summary="$RUN_DIR/builder-summary.md"
{
  echo "# RS-007 Builder Diff Summary"
  echo
  echo "Generated UTC: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "Root: $ROOT"
  echo "Base dir: $BASE_DIR"
  echo
} > "$summary"

for b in "${builders[@]}"; do
  wt="$BASE_DIR/$b"
  diff_file="$RUN_DIR/builder-diffs/$b.diff"
  stat_file="$RUN_DIR/builder-diffs/$b.status.txt"
  echo "## $b" >> "$summary"
  if [[ ! -d "$wt" ]]; then
    echo "MISSING worktree: $wt" | tee "$stat_file" >> "$summary"
    echo >> "$summary"
    continue
  fi
  {
    echo "# $b"
    echo "worktree: $wt"
    echo
    git -C "$wt" status --short
  } > "$stat_file"
  git -C "$wt" diff --stat >> "$summary" || true
  echo >> "$summary"
  git -C "$wt" diff > "$diff_file" || true
  if [[ ! -s "$diff_file" ]]; then
    echo "(no diff)" > "$diff_file"
  fi
  echo "Diff: $diff_file" >> "$summary"
  echo >> "$summary"
done

cat > "$RUN_DIR/tasks/common-review.md" <<'EOF'
You are a Ralph reviewer for Brivva RS-007. Read-only.

Goal: attack builder diffs for concrete failure modes. Do not edit files.

Rules:
- Be terse.
- No broad rewrites.
- No style bikeshedding.
- Prefer P0/P1 production blockers.
- If finding is not actionable, omit it.
- Max 12 findings.

Each finding format:
- Severity: P0/P1/P2
- Area: file/section or builder
- Failure: what breaks
- Why: launch impact
- Minimal fix: exact small action

Context:
- `temp.md` concise RS-007 plan
- `plan.md` full RS-007 plan
- `tmp/rs007-review-swarm/latest/builder-summary.md`
- `tmp/rs007-review-swarm/latest/builder-diffs/*.diff`
EOF

make_ralph_task() {
  local r="$1"
  local task="$RUN_DIR/tasks/$r.md"
  cat "$RUN_DIR/tasks/common-review.md" > "$task"
  echo >> "$task"
  echo "# Focus: $r" >> "$task"
  echo >> "$task"
  case "$r" in
    security-secret-critic)
      cat >> "$task" <<'EOF'
Focus only on:
- prod secret mutation risk
- leaking stream keys/API keys/log secrets
- dangerous AWS mutating commands
- missing guardrails around Infisical/Secrets Manager
EOF
      ;;
    aws-network-critic)
      cat >> "$task" <<'EOF'
Focus only on:
- private no-public-IP GPU networking
- VPC endpoints vs NAT confusion
- operator/browser access path
- real-platform proof vs fake-sink proof
- region/AZ/quota/capacity assumptions
EOF
      ;;
    cost-rollback-critic)
      cat >> "$task" <<'EOF'
Focus only on:
- GPU spend left running
- rollback command gaps
- primary service blast radius
- accidental deploy/cutover risk
- missing zero-spend verification
EOF
      ;;
    observability-critic)
      cat >> "$task" <<'EOF'
Focus only on:
- CloudWatch logs/export gaps
- missing grep signals
- inability to prove FFmpeg/NVENC/fonts/platform health
- weak artifacts/verdict templates
EOF
      ;;
    billing-critic)
      cat >> "$task" <<'EOF'
Focus only on:
- provider failure window persistence
- unbillable window subtraction
- sibling healthy outputs billable
- billing tests/scripts/checklist gaps
EOF
      ;;
    operator-launch-critic)
      cat >> "$task" <<'EOF'
Focus only on:
- human launch-day steps too complex
- unclear go/no-go gates
- missing manual dashboard checks
- commands that can be run in wrong order
- missing one-shot Grip key warnings
EOF
      ;;
  esac
}

for r in "${ralphs[@]}"; do
  make_ralph_task "$r"
done

cat > "$RUN_DIR/tasks/judge.md" <<EOF
You are RS-007 Judge. Read-only unless explicitly asked later.

Inputs:
- Builder summary: tmp/rs007-review-swarm/latest/builder-summary.md
- Builder diffs: tmp/rs007-review-swarm/latest/builder-diffs/*.diff
- Ralph reviews: tmp/rs007-review-swarm/latest/reviews/*.md
- Plans: temp.md, plan.md, docs/rust-stress-test/tracker.md

Task:
1. Deduplicate Ralph findings.
2. Classify as P0 blocker / P1 should fix / P2 nice.
3. Map each finding to builder worktree(s) responsible.
4. Produce merge order for builder patches.
5. Produce exact orchestrator commands to inspect/apply patches.
6. Say which patches are safe to merge now, which need revision, which to discard.

Do not edit files. Do not update tracker.
EOF

cat > "$RUN_DIR/README.md" <<EOF
# RS-007 Review Swarm Run

Run dir: $RUN_DIR

Files:
- builder-summary.md
- builder-diffs/*.diff
- tasks/*.md
- reviews/*.md
- judge.md (after judge runs)

Attach tmux:

\`\`\`bash
tmux attach -t $SESSION
\`\`\`
EOF

if [[ "$START_PI" != "1" ]]; then
  echo "Prepared review swarm only: $RUN_DIR"
  exit 0
fi

if tmux has-session -t "$SESSION" 2>/dev/null; then
  if [[ "$FORCE" == "1" ]]; then
    tmux kill-session -t "$SESSION"
  else
    echo "tmux session exists: $SESSION (attach: tmux attach -t $SESSION, or FORCE=1 recreate)" >&2
    exit 1
  fi
fi

first=1
for r in "${ralphs[@]}"; do
  task="$RUN_DIR/tasks/$r.md"
  out="$RUN_DIR/reviews/$r.md"
  cmd="cd '$ROOT' && $PI_BIN $MODEL_ARGS -p @$task | tee '$out'; echo; echo '[DONE] $r -> $out'; exec bash"
  if [[ "$first" == "1" ]]; then
    tmux new-session -d -s "$SESSION" -n "$r" "$cmd"
    first=0
  else
    tmux new-window -t "$SESSION" -n "$r" "$cmd"
  fi
done

# Judge waits for all review files to appear non-empty, then runs.
judge_task="$RUN_DIR/tasks/judge.md"
judge_out="$RUN_DIR/judge.md"
wait_cmd="cd '$ROOT'; echo 'Waiting for ${#ralphs[@]} reviews in $RUN_DIR/reviews ...'; while true; do n=0; for f in '$RUN_DIR'/reviews/*.md; do [[ -s \"\$f\" ]] && n=\$((n+1)); done; echo \"reviews ready: \$n/${#ralphs[@]}\"; [[ \$n -ge ${#ralphs[@]} ]] && break; sleep 20; done; $PI_BIN $MODEL_ARGS -p @$judge_task | tee '$judge_out'; echo; echo '[DONE] judge -> $judge_out'; exec bash"
tmux new-window -t "$SESSION" -n "judge" "$wait_cmd"

echo "Launched review tmux session: $SESSION"
echo "Attach: tmux attach -t $SESSION"
echo "Run dir: $RUN_DIR"
echo "Latest: $LATEST_LINK"
