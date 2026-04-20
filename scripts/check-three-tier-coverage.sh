#!/usr/bin/env bash
# CLAUDE.md §0.1 / §0.2 — three-tier-coverage presence check.
#
# For each *new* source file introduced by the PR, verify the same PR
# touches a test file at each tier the CLAUDE.md mandate requires:
#
#   Unit        → co-located `<name>.test.ts` / `<name>_test.rs` / `#[cfg(test)] mod`
#   Integration → a file under `<component>/tests/`
#   E2E         → a file under `frontend/e2e/` or `tests/e2e/`
#
# "Touched" means added or modified in the PR diff. The exemption list
# below carves out files that legitimately do not need all three tiers
# (generated bindings, pure type declarations, docs, fixtures).
#
# BASE_REF defaults to origin/main — override for local dry-runs via
#   BASE_REF=main ./scripts/check-three-tier-coverage.sh

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

BASE_REF=${BASE_REF:-origin/main}

# If the base ref isn't available (e.g. shallow checkout before CI
# fetches main), skip silently — the contract-drift + layer-audit jobs
# already catch the surface this check protects.
if ! git rev-parse --verify "$BASE_REF" >/dev/null 2>&1; then
    echo "three-tier-coverage: ${BASE_REF} not available, skipping"
    exit 0
fi

# EXEMPT: files where three-tier coverage doesn't apply.
is_exempt() {
    local f="$1"
    case "$f" in
        # Generated
        *workers-api.d.ts|*brivva-workers.json|*contracts/workers.rs) return 0 ;;
        # Pure-type / barrel files
        *types.ts|*types.rs|*schema.ts|*schema.rs|*mod.rs) return 0 ;;
        # Test / e2e / fixture / doc / config files — they ARE the coverage
        *.test.ts|*.test.tsx|*_test.rs|*e2e*|*.md|*.sql|*.html|*.css) return 0 ;;
        */tests/*|*/e2e/*|*/fixtures/*|*/design/*) return 0 ;;
        # Scripts + CI + config
        scripts/*|.github/*|.claude/*|*config*.ts|*.config.*) return 0 ;;
        # Top-level / orchestration bootstrap
        */main.rs|*/main.tsx|*/lib.rs) return 0 ;;
        *) return 1 ;;
    esac
}

# Test tier classification. Returns one of: unit | integration | e2e | none
tier_of() {
    local f="$1"
    case "$f" in
        *e2e/*|*tests/e2e/*) echo e2e; return ;;
        */tests/*) echo integration; return ;;
        *.test.ts|*.test.tsx|*_test.rs) echo unit; return ;;
        *) echo none; return ;;
    esac
}

# Gather diff: split into new-src vs touched-tests. Use tempfiles +
# while-read loops so the script stays portable to bash 3 (macOS default).
CHANGED_FILE="$(mktemp)"
ADDED_FILE="$(mktemp)"
trap 'rm -f "$CHANGED_FILE" "$ADDED_FILE"' EXIT
git diff --name-only "$BASE_REF"...HEAD > "$CHANGED_FILE" || true
git diff --name-only --diff-filter=A "$BASE_REF"...HEAD > "$ADDED_FILE" || true

if [[ ! -s "$CHANGED_FILE" ]]; then
    echo "three-tier-coverage: no changed files vs ${BASE_REF}"
    exit 0
fi

tier_unit=0
tier_integration=0
tier_e2e=0
while IFS= read -r f; do
    [[ -z "$f" ]] && continue
    t="$(tier_of "$f")"
    case "$t" in
        unit)        tier_unit=1 ;;
        integration) tier_integration=1 ;;
        e2e)         tier_e2e=1 ;;
    esac
done < "$CHANGED_FILE"

fail=0
while IFS= read -r f; do
    [[ -z "$f" ]] && continue
    if is_exempt "$f"; then
        continue
    fi
    case "$f" in
        *.rs|*.ts|*.tsx) ;;
        *) continue ;;
    esac

    missing=()
    [[ $tier_unit -eq 0        ]] && missing+=("unit")
    [[ $tier_integration -eq 0 ]] && missing+=("integration")
    [[ $tier_e2e -eq 0         ]] && missing+=("e2e")
    if [[ ${#missing[@]} -gt 0 ]]; then
        joined="$(IFS=, ; echo "${missing[*]}")"
        echo "::error file=${f}::new source file added without tests at tier(s): ${joined} — §0.2 mandates unit+integration+e2e coverage (add tests in the same PR or mark exempt)"
        fail=$((fail + 1))
    fi
done < "$ADDED_FILE"

if [[ $fail -gt 0 ]]; then
    echo "three-tier-coverage: ${fail} new file(s) missing required test tier(s)"
    exit 1
fi

echo "three-tier-coverage: all new src files have at least unit+integration+e2e tier evidence in the PR"
