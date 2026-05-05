#!/usr/bin/env bash
# Spawn isolated pi subagents for RS-007 implementation work.
# Creates git worktrees + task prompts, then launches one tmux window per agent.

set -euo pipefail

ROOT="$(git rev-parse --show-toplevel)"
BASE_BRANCH="$(git -C "$ROOT" branch --show-current)"
BASE_DIR="${BASE_DIR:-$(dirname "$ROOT")/brivva-rs007-agents}"
TASK_DIR="$ROOT/tmp/rs007-agent-tasks"
SESSION="${SESSION:-rs007-agents}"
PI_BIN="${PI_BIN:-pi}"
MODEL_ARGS="${MODEL_ARGS:-}"
START_PI="${START_PI:-1}"
START_ORCHESTRATOR="${START_ORCHESTRATOR:-1}"
FORCE="${FORCE:-0}"

cd "$ROOT"

if ! command -v "$PI_BIN" >/dev/null 2>&1; then
  echo "pi not found. Set PI_BIN=/path/to/pi" >&2
  exit 1
fi

if ! command -v tmux >/dev/null 2>&1 && [[ "$START_PI" == "1" ]]; then
  echo "tmux not found. Install tmux or run START_PI=0 $0" >&2
  exit 1
fi

mkdir -p "$TASK_DIR" "$BASE_DIR"

agents=(
  "infra-auditor"
  "runtime-self-check"
  "aws-smoke-entrypoint"
  "secrets-isolation"
  "log-export-verdict"
  "billing-drill"
)

write_common() {
  cat <<'EOF'
You are a Brivva RS-007 subagent working inside an isolated git worktree.

Hard rules:
- Do NOT run paid AWS scale-up, deploy, or real platform stream commands.
- Do NOT mutate Infisical prod values, AWS Secrets Manager prod secret, `.env`, or `.dev.vars`.
- Do NOT mark RS-007 patched/watch in tracker unless orchestrator explicitly asks.
- Fake-sink/private GPU proof is NOT production proof.
- Prefer narrow patches. Do not broad-refactor unrelated code.
- Return: files changed, commands run, evidence, risks, exact next orchestrator command.

Context files to read first:
- temp.md
- plan.md
- docs/rust-stress-test/tracker.md
- infra/README.md when touching AWS infra/scripts

Before editing:
- run `pwd && git status --short && git branch --show-current`

After editing:
- run relevant tests/checks if safe/local
- run `git diff --check`
- do not commit unless explicitly instructed
EOF
}

make_task() {
  local name="$1"
  local file="$TASK_DIR/${name}.md"
  {
    write_common
    echo
    echo "# Task: $name"
    echo
    case "$name" in
      infra-auditor)
        cat <<'EOF'
Goal: make AWS/network prerequisites executable and explicit.

Allowed focus:
- infra docs/scripts only: `infra/**`, `scripts/**`, `docs/**`, `plan.md`, `temp.md`
- no Terraform apply/deploy

Tasks:
1. Audit current Terraform GPU network path: no-public-IP, VPC endpoints, NAT gap, operator/browser access gap.
2. Replace missing README helper-script assumptions with scripts or exact AWS CLI commands.
3. Add a safe preflight script if useful. It may run read-only AWS commands only when user later executes it; do not execute AWS here.
4. Ensure all AWS commands are region-explicit or use `AWS_REGION=us-east-1`.
5. Document exact blockers for real-platform smoke vs fake-sink smoke.

Deliver evidence with file paths and command examples.
EOF
        ;;
      runtime-self-check)
        cat <<'EOF'
Goal: remove ECS Exec dependency by adding startup/runtime self-check proof.

Allowed focus:
- `server-rs/**`
- minimal docs/scripts updates if needed

Tasks:
1. Add startup self-check logs or a safe diagnostic path that proves:
   - ffmpeg version/config includes OpenSSL/native RTMPS path
   - protocols include `rtmp`/`rtmps`
   - filters include `drawtext`
   - encoders include `h264_nvenc` when `BRIVVA_VIDEO_ENCODER=nvenc`
   - `fc-match "Noto Sans CJK KR"` resolves
2. Avoid shelling on every request; startup once is preferred.
3. Redact/no secrets.
4. Add tests where practical; otherwise add grep checklist.

Deliver exact log lines orchestrator should grep in CloudWatch.
EOF
        ;;
      aws-smoke-entrypoint)
        cat <<'EOF'
Goal: create a safe AWS-side smoke entrypoint/mechanism.

Allowed focus:
- `server-rs/**`, `scripts/**`, `docs/**`

Tasks:
1. Find current local MP4 smoke assumptions.
2. Add or document a way to run equivalent fake/local sink smoke from AWS task/rehearsal path without real platform keys.
3. If implementation is too large, create a script/checklist wrapper that prepares run folder and prints exact commands, but does not execute paid AWS actions.
4. Output pass/fail grep patterns: FFmpeg restart, below realtime, media drops, TTS overflow, hard recovery.

Deliver safe command(s) for orchestrator to run later.
EOF
        ;;
      secrets-isolation)
        cat <<'EOF'
Goal: make bad-provider drills impossible to poison prod secrets.

Allowed focus:
- `infra/**`, `scripts/**`, `server-rs/**`, `docs/**`

Tasks:
1. Inspect how `brivva/env` is wired to ECS.
2. Design/implement isolated rehearsal secret or ECS task override path for bad Soniox/ElevenLabs drills.
3. Do not mutate existing prod secret values.
4. Add guardrails/docs/scripts so drill commands visibly refuse to touch shared prod secret.

Deliver proof that primary `brivva` cannot be affected by drill setup.
EOF
        ;;
      log-export-verdict)
        cat <<'EOF'
Goal: automate run artifact capture.

Allowed focus:
- `scripts/**`, `docs/**`, `plan.md`, `temp.md`

Tasks:
1. Create run folder skeleton generator under `tmp/aws-soak-runs/...`.
2. Create CloudWatch log export helper by time range/service prefix.
3. Generate `manifest.md`, `platform-visible-live.md`, `billing-check.md`, `verdict.md` templates.
4. Do not call AWS mutating APIs.

Deliver example invocation and generated file list.
EOF
        ;;
      billing-drill)
        cat <<'EOF'
Goal: define and/or automate billing verification for RS-006/RS-007 drills.

Allowed focus:
- `workers/**`, `server-rs/**`, `scripts/**`, `docs/**`

Tasks:
1. Inspect provider failure/billing code and tests.
2. Add a local/read-only verification script or checklist for:
   - bad Soniox unbillable source/lang window
   - bad ElevenLabs unbillable TTS/lang window
   - bad RTMP/platform unbillable output window
   - healthy siblings remain billable
3. Add tests if gaps are obvious and bounded.
4. Do not require real AWS/platform to run unit checks.

Deliver exact API/test commands for orchestrator.
EOF
        ;;
    esac
  } > "$file"
}

for agent in "${agents[@]}"; do
  make_task "$agent"
done

# Orchestrator task notes.
{
  echo "# RS-007 Orchestrator"
  echo
  echo "Session: $SESSION"
  echo "Base branch: $BASE_BRANCH"
  echo "Base dir: $BASE_DIR"
  echo
  echo "Workflow:"
  echo "1. Wait for subagents to finish."
  echo '2. Review each worktree: `git -C <worktree> diff --stat && git -C <worktree> diff`.'
  echo "3. Cherry-pick/apply safe patches into main repo one at a time."
  echo "4. Run tests/checks after each merge."
  echo '5. Update `docs/rust-stress-test/tracker.md` only after evidence.'
  echo
  echo "Worktrees:"
  for agent in "${agents[@]}"; do
    echo "- $agent: $BASE_DIR/$agent"
  done
} > "$TASK_DIR/00-orchestrator.md"

create_worktree() {
  local agent="$1"
  local wt="$BASE_DIR/$agent"
  local branch="agent/rs007-$agent"

  if [[ -d "$wt/.git" || -f "$wt/.git" ]]; then
    if [[ "$FORCE" == "1" ]]; then
      git worktree remove --force "$wt" || true
    else
      echo "worktree exists: $wt (set FORCE=1 to recreate)"
      return
    fi
  fi

  if git show-ref --verify --quiet "refs/heads/$branch"; then
    if [[ "$FORCE" == "1" ]]; then
      git branch -D "$branch"
    else
      git worktree add "$wt" "$branch"
      return
    fi
  fi

  git worktree add -b "$branch" "$wt" "$BASE_BRANCH"

  # Copy current in-progress plan context into worktree so agents see latest draft.
  cp "$ROOT/plan.md" "$wt/plan.md" 2>/dev/null || true
  cp "$ROOT/temp.md" "$wt/temp.md" 2>/dev/null || true
  mkdir -p "$wt/tmp/rs007-agent-tasks"
  cp "$TASK_DIR/$agent.md" "$wt/tmp/rs007-agent-tasks/$agent.md"
}

for agent in "${agents[@]}"; do
  create_worktree "$agent"
done

if [[ "$START_PI" != "1" ]]; then
  echo "Prepared tasks/worktrees only."
  echo "Tasks: $TASK_DIR"
  echo "Worktrees: $BASE_DIR"
  exit 0
fi

if tmux has-session -t "$SESSION" 2>/dev/null; then
  if [[ "$FORCE" == "1" ]]; then
    tmux kill-session -t "$SESSION"
  else
    echo "tmux session exists: $SESSION (attach: tmux attach -t $SESSION, or FORCE=1 recreate)"
    exit 1
  fi
fi

first=1
if [[ "$START_ORCHESTRATOR" == "1" ]]; then
  orch_cmd="cd '$ROOT' && $PI_BIN $MODEL_ARGS @tmp/rs007-agent-tasks/00-orchestrator.md; exec bash"
  tmux new-session -d -s "$SESSION" -n "orchestrator" "$orch_cmd"
  first=0
fi

for agent in "${agents[@]}"; do
  wt="$BASE_DIR/$agent"
  task="tmp/rs007-agent-tasks/$agent.md"
  cmd="cd '$wt' && $PI_BIN $MODEL_ARGS -p @$task; echo; echo '[DONE] $agent'; exec bash"
  if [[ "$first" == "1" ]]; then
    tmux new-session -d -s "$SESSION" -n "$agent" "$cmd"
    first=0
  else
    tmux new-window -t "$SESSION" -n "$agent" "$cmd"
  fi
done

echo "Launched tmux session: $SESSION"
echo "Attach: tmux attach -t $SESSION"
echo "Tasks: $TASK_DIR"
echo "Worktrees: $BASE_DIR"
