#!/usr/bin/env bash
# Runs the 4 PR-delta gate checks locally. Mirrors what
# .github/workflows/pre-merge-gate.yml executes on every PR.
#
# Exit 0 if all green; exit 1 on the first failing check. Use
#   BASE_REF=main bash scripts/run-pre-merge-gate.sh
# to check against a branch other than origin/main.

set -euo pipefail
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

echo "── §5.1 file-size ──"
bash scripts/check-file-size.sh

echo
echo "── §0.5.1 fixture-presence ──"
bash scripts/check-fixture-presence.sh

echo
echo "── §0.5.4 silent-path log audit ──"
bash scripts/check-silent-path-logs.sh

echo
echo "── §0.2 three-tier-coverage ──"
bash scripts/check-three-tier-coverage.sh

echo
echo "pre-merge gate: all 4 delta checks green"
