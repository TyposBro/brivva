#!/usr/bin/env bash
# Dependency-direction audit for CLAUDE.md §1.2 applied to server-rs/src.
# Each layer may only import from layers closer to core. Violations exit 1.
#
# Pure Rust is already mostly self-enforcing via `pub`/`pub(crate)` visibility,
# but the layered architecture's "core does not know about features" rule is
# stricter than the language's privacy model — hence this greps for
# `use crate::<layer>::` paths and checks direction.

set -euo pipefail

cd "$(dirname "$0")/.."
ROOT="src"

fail=0

# core/ may not import shared/features/orchestration.
out=$(grep -rnE "^use crate::(shared|features|orchestration)" "$ROOT/core" 2>/dev/null || true)
if [ -n "$out" ]; then
  echo "core may not import from shared/features/orchestration:"
  echo "$out"
  fail=1
fi

# shared/ may not import features/orchestration.
out=$(grep -rnE "^use crate::(features|orchestration)" "$ROOT/shared" 2>/dev/null || true)
if [ -n "$out" ]; then
  echo "shared may not import from features/orchestration:"
  echo "$out"
  fail=1
fi

# features/ may not import orchestration.
out=$(grep -rnE "^use crate::orchestration" "$ROOT/features" 2>/dev/null || true)
if [ -n "$out" ]; then
  echo "features may not import from orchestration:"
  echo "$out"
  fail=1
fi

# Cross-feature imports (features/<a> importing features/<b>).
while IFS= read -r -d '' dir; do
  name=$(basename "$dir")
  out=$(grep -rnE "^use crate::features::" "$dir" 2>/dev/null | grep -vE "crate::features::$name" || true)
  if [ -n "$out" ]; then
    echo "feature $name may not import another feature:"
    echo "$out"
    fail=1
  fi
done < <(find "$ROOT/features" -mindepth 1 -maxdepth 1 -type d -print0)

if [ "$fail" -eq 0 ]; then
  echo "[check-layers] 0 violations — dependency graph matches CLAUDE.md §1.2"
fi
exit "$fail"
