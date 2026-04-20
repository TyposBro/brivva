#!/usr/bin/env bash
# CLAUDE.md §0.5.1 — real-response fixture presence check.
#
# Every external vendor whose API server-rs or workers deserializes MUST
# have at least one non-empty JSON fixture under its tests/fixtures
# subdirectory. An empty directory means a fixture-roundtrip drift could
# land silently; the vendor list below is the rule.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

VENDORS=(
    "workers/tests/fixtures/elevenlabs"
    "workers/tests/fixtures/stripe"
    "workers/tests/fixtures/youtube"
    "workers/tests/fixtures/google-oauth"
    "workers/tests/fixtures/grip"
    "server-rs/tests/fixtures/soniox"
)

fail=0
for dir in "${VENDORS[@]}"; do
    if [[ ! -d "$dir" ]]; then
        echo "::error::${dir} missing — §0.5.1 requires a fixture dir per vendor"
        fail=$((fail + 1))
        continue
    fi
    count="$(find "$dir" -maxdepth 1 -type f -name '*.json' | wc -l | tr -d ' ')"
    if [[ "$count" -eq 0 ]]; then
        echo "::error::${dir} has 0 fixtures — §0.5.1 requires ≥1 captured response per vendor"
        fail=$((fail + 1))
    else
        # Reject fixtures that are 0-byte or `null` placeholders. An empty
        # object / empty array is valid (e.g. Soniox mid-stream frame with
        # no tokens) — don't flag those.
        while IFS= read -r f; do
            content="$(tr -d ' \t\r\n' < "$f")"
            if [[ -z "$content" || "$content" == "null" ]]; then
                echo "::error file=${f}::fixture is empty / null placeholder — capture a real response"
                fail=$((fail + 1))
            fi
        done < <(find "$dir" -maxdepth 1 -type f -name '*.json')
    fi
done

if [[ $fail -gt 0 ]]; then
    echo "fixture-presence: ${fail} violation(s)"
    exit 1
fi

echo "fixture-presence: all ${#VENDORS[@]} vendor dirs populated"
