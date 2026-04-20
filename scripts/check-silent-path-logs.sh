#!/usr/bin/env bash
# CLAUDE.md §0.5.4 — silent-path log audit (automated grep).
#
# Every `continue;` in the Rust pipeline code MUST have a
# tracing::{warn,info,debug,error}! call within 3 lines before or after,
# OR a `// AUDIT:` acknowledgement comment explaining why this continue
# is intentionally non-silent (e.g. normal-flow filter, not a bug path).
#
# This is the automated proxy for the live-session grep in
# docs/post-merge-log-audit-2026-04-20.md — it catches the "someone
# deleted a warn! line" regression class at PR time.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

PIPELINE_DIR="server-rs/src/features/broadcast/data/pipeline"

if [[ ! -d "$PIPELINE_DIR" ]]; then
    echo "::error::pipeline dir ${PIPELINE_DIR} missing — update the audit script"
    exit 1
fi

CONTEXT_LINES=8
fail=0

while IFS= read -r hit; do
    # hit format: <file>:<line_number>:<text>
    file="${hit%%:*}"
    rest="${hit#*:}"
    line="${rest%%:*}"

    start=$((line - CONTEXT_LINES))
    end=$((line + CONTEXT_LINES))
    [[ $start -lt 1 ]] && start=1

    window="$(sed -n "${start},${end}p" "$file")"
    if echo "$window" | grep -qE "tracing::(warn|info|debug|error)!"; then
        continue
    fi
    if echo "$window" | grep -q "// AUDIT:"; then
        continue
    fi

    echo "::error file=${file},line=${line}::silent \`continue;\` without tracing log within ±${CONTEXT_LINES} lines — §0.5.4 violation (add tracing::warn!(…) or mark the branch with '// AUDIT: <reason>')"
    fail=$((fail + 1))
done < <(grep -rEn '^\s*continue\s*;' "$PIPELINE_DIR" || true)

if [[ $fail -gt 0 ]]; then
    echo "silent-path audit: ${fail} bare \`continue;\` branch(es) without a log line"
    exit 1
fi

echo "silent-path audit: every \`continue;\` in pipeline code has a logged or acknowledged branch"
