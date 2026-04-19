#!/usr/bin/env bash
#
# install-hooks.sh — point git at the versioned hooks in this repo.
#
# Run once per clone. Safe to re-run.
#
# Why `core.hooksPath` instead of copying to `.git/hooks/`:
#   - hooks are version-controlled, diff-reviewable, shared across machines
#   - no drift between developer checkouts
#   - one command to uninstall: `git config --unset core.hooksPath`

set -euo pipefail

REPO_ROOT=$(git rev-parse --show-toplevel 2>/dev/null || { echo "not a git repo"; exit 1; })
HOOKS_DIR="$REPO_ROOT/scripts/git-hooks"

if [ ! -d "$HOOKS_DIR" ]; then
  echo "hooks directory missing: $HOOKS_DIR"
  exit 1
fi

# Make every hook executable (mostly a no-op, but safe on fresh clones).
chmod +x "$HOOKS_DIR"/*

# Point git at the repo-versioned hooks dir.
git -C "$REPO_ROOT" config core.hooksPath scripts/git-hooks

echo "git hooks installed."
echo "  pre-commit   — fast structural checks (layer rules, secrets, fmt)"
echo "  pre-push     — contract drift + typecheck + cargo check"
echo "  post-merge   — informational reminders after pull"
echo ""
echo "Skip a hook for one commit/push:  --no-verify"
echo "Uninstall:                        git config --unset core.hooksPath"
