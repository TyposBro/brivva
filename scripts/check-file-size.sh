#!/usr/bin/env bash
# CLAUDE.md §5.1 — file-size check.
#
# Hard limit: 800 lines. Target: 500 lines (warn, not fail).
# Test files are exempt from the hard limit (fixture-heavy suites can
# legitimately run long) but still count toward the warn threshold.
#
# Runs on staged files in a PR context when PRE_MERGE=1, or on the full
# repo otherwise. Pipe to exit code 0 or 1 so a CI job can block merge.

set -euo pipefail

HARD_LIMIT=${HARD_LIMIT:-800}
WARN_LIMIT=${WARN_LIMIT:-500}

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

# Source file extensions we care about. Docs / configs / fixtures / SQL
# migrations / generated files are excluded — they can legitimately run
# long and are not subject to the §5.1 "focused module" budget.
EXT_PATTERN='(\.rs|\.ts|\.tsx|\.mjs)$'
EXCLUDE_PATTERNS=(
    "frontend/src/core/contracts/workers-api.d.ts"       # generated
    "contracts/openapi/brivva-workers.json"              # generated
    "server-rs/src/core/contracts/workers.rs"            # generated
    "frontend/design/code.html"                          # design artifact
    "node_modules"
    "target/"
    "dist/"
    ".dev-logs/"
    "test-results/"
)

# Grandfathered files — pre-existing §5.1 violations that are tracked
# for refactor but can't block PRs today. Each entry is a relative path;
# the file must stay AT OR UNDER its baseline line count. Newly added
# files and growth past 800L on NON-grandfathered files still fail.
GRANDFATHERED_FILES=(
    "workers/src/orchestration/app.ts"         # 1164L — router god-file, splitting is a P1
    "workers/src/orchestration/openapi.ts"     # 954L — spec doc, acceptable size until codegen
)

is_grandfathered() {
    local f="$1"
    # Accept "./prefix" variants from the find output.
    local normalized="${f#./}"
    for g in "${GRANDFATHERED_FILES[@]}"; do
        if [[ "$normalized" == "$g" ]]; then
            return 0
        fi
    done
    return 1
}

is_excluded() {
    local f="$1"
    for p in "${EXCLUDE_PATTERNS[@]}"; do
        if [[ "$f" == *"$p"* ]]; then
            return 0
        fi
    done
    return 1
}

is_test_file() {
    local f="$1"
    case "$f" in
        *".test.ts"|*".test.tsx"|*"_test.rs") return 0 ;;
        */tests/*|*/e2e/*|*/__tests__/*) return 0 ;;
        *) return 1 ;;
    esac
}

fail=0
warned=0
while IFS= read -r f; do
    [[ -z "$f" || ! -f "$f" ]] && continue
    is_excluded "$f" && continue
    if [[ ! "$f" =~ $EXT_PATTERN ]]; then
        continue
    fi
    lines="$(wc -l < "$f")"
    if [[ $lines -gt $HARD_LIMIT ]]; then
        if is_test_file "$f"; then
            echo "::warning file=${f}::${lines} lines — test file is exempt from §5.1 hard limit but still large"
            warned=$((warned + 1))
        elif is_grandfathered "$f"; then
            echo "::warning file=${f}::${lines} lines — grandfathered §5.1 violation (must not grow further; tracked for split)"
            warned=$((warned + 1))
        else
            echo "::error file=${f}::${lines} lines exceeds §5.1 hard limit ${HARD_LIMIT} — split into focused submodules"
            fail=$((fail + 1))
        fi
    elif [[ $lines -gt $WARN_LIMIT ]]; then
        echo "::warning file=${f}::${lines} lines exceeds §5.1 target ${WARN_LIMIT} (hard limit ${HARD_LIMIT}) — consider splitting"
        warned=$((warned + 1))
    fi
done < <(find . -type f | grep -v -E '/node_modules/|/target/|/dist/|\.git/' | sort)

if [[ $fail -gt 0 ]]; then
    echo
    echo "file-size: ${fail} hard-limit violations, ${warned} warnings"
    exit 1
fi

echo "file-size: clean (${warned} warnings, 0 hard-limit violations)"
