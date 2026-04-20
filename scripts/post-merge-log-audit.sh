#!/usr/bin/env bash
# CLAUDE.md §0.5.4 — post-merge silent-path log audit runner.
#
# Spins up the dev stack, tees all stdout+stderr to a timestamped log
# file, and prompts the operator to walk a real 30-min session
# end-to-end. When the session ends, the audit doc at
# docs/post-merge-log-audit-2026-04-20.md has the grep commands to
# verify every silent branch actually fired during the run.
#
# Usage:
#   bash scripts/post-merge-log-audit.sh
#
# Prereqs:
#   - wrangler logged in (for Workers tailing)
#   - soniox + elevenlabs keys in server-rs/.env
#   - a real Grip or YouTube destination to push to

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

TS="$(date +%Y%m%dT%H%M%S)"
LOG="/tmp/brivva-audit-${TS}.log"
AUDIT_DOC="docs/post-merge-log-audit-2026-04-20.md"

echo "─── §0.5.4 log audit — ${TS} ───"
echo "log file: ${LOG}"
echo "audit doc: ${AUDIT_DOC}"
echo

if [[ ! -f "${AUDIT_DOC}" ]]; then
    echo "error: audit doc ${AUDIT_DOC} missing — regenerate before running" >&2
    exit 1
fi

cat <<'PROMPT'
Walkthrough to cover every §0.5.4 branch:

  1. Sign in with Google (exercises W8).
  2. Open /dashboard, connect YouTube (exercises W7 on any OAuth error).
  3. Record a voice clone (exercises voice-upsert path + W1 if re-record).
  4. Create a session with 2 target langs, pick a passthrough + a real
     destination (exercises Grip/YouTube provision branches W2 / W3 / W4).
  5. Start live. Speak continuously for ~30 minutes.
  6. Mid-session: pause speaking for ~10s (stt idle), then speak again
     (exercises soniox reconnect path R10).
  7. Mid-session: force a brief network drop (wifi off/on) for ~5s
     (exercises R8 / R9 / R10 reconnect telemetry).
  8. End session. Verify post-stream summary loads (exercises W5 / W6
     if target_langs JSON ever drifts).

After: run the grep commands in ${AUDIT_DOC} against ${LOG}.
PROMPT
echo
read -rp "press enter when ready to spin up the stack, ctrl+c to abort … "

# Tee the dev stack to the log file. `script` captures an accurate
# transcript of stdout+stderr including terminal colors + interactive
# output; wrangler tail logs land here too once the stack brings them in.
if command -v script >/dev/null 2>&1; then
    # BSD script on macOS: `script <file> <cmd>`; Linux: `script -q -c <cmd> <file>`.
    if [[ "$(uname)" == "Darwin" ]]; then
        script -q "${LOG}" bash scripts/dev-stack.sh
    else
        script -q -c "bash scripts/dev-stack.sh" "${LOG}"
    fi
else
    # Fallback — less reliable for colors but still captures the stream.
    bash scripts/dev-stack.sh 2>&1 | tee "${LOG}"
fi

echo
echo "─── stack exited. Verify the audit doc now. ───"
echo "cat ${LOG} | head  # sanity-check capture"
echo "grep -c 'dispatching translated text' ${LOG}"
echo "  ... and every other row in ${AUDIT_DOC}"
