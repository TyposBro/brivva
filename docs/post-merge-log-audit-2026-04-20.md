# §0.5.4 Silent-Path Log Audit — 2026-04-20

Per CLAUDE.md §0.5.4: every silent branch (every `continue`, every
`if let Ok(_) = ... { /* nothing */ }`, every error-swallowing fallback,
every background-loop idle/skip) MUST emit a structured tracing event
that is greppable in production logs with enough context (session_id,
lang, stream_id, etc.) to trace the flow.

This doc is the post-merge audit the rule requires:

1. Enumerate every silent branch across the STT / translate / TTS
   pipeline + Workers fallback paths.
2. Note the expected log line for each.
3. Verify coverage against:
   - **unit-test log capture** where the branch has a unit test (built-in
     `tracing_subscriber::fmt::test` or the in-file assertions).
   - **live-session grep** — Aziz must run a 30-min real session
     (local / staging) and grep the captured log file for each row.
     Rows marked `LIVE_PENDING` need that run to close.

Run instructions for the live pass at the bottom.

---

## server-rs silent branches

All rows use `tracing::` (converted from prior `eprintln!` during this
audit — an `eprintln!` only reaches stderr, so it was NOT greppable via
the structured JSON log pipeline and failed §0.5.4 at audit time).

| # | File:Line | Branch | Expected log (grep key) | Status |
|---|-----------|--------|------------------------|--------|
| R1 | `pipeline/stt.rs:30` | `SONIOX_API_KEY` empty → return | `"SONIOX_API_KEY not set — STT pipeline disabled"` | COVERED (warn at startup) |
| R2 | `pipeline/stt.rs:62` | fan-out channel full → source task drop | `"stt fan-out dropped chunk to source task"` | COVERED (debug) |
| R3 | `pipeline/stt.rs:70` | fan-out channel full → target task drop | `"stt fan-out dropped chunk to target task"` | COVERED (debug) |
| R4 | `pipeline/stt.rs:135` | `connect_soniox` returned `None` → outer loop exit | `"soniox connect failed after max reconnects"` in `connect_soniox` | COVERED (error) |
| R5 | `pipeline/stt.rs:150` | config-send failed, exceeded max reconnects | `"stt config-send failed; exceeded max reconnects"` | COVERED (error) |
| R6 | `pipeline/stt.rs:158` | config-send failed, will retry | `"stt config-send failed; looping to reconnect"` | COVERED (warn) |
| R7 | `pipeline/stt.rs:189` | clean close path | `"stt session ended cleanly"` | COVERED (info) |
| R8 | `pipeline/stt.rs:197` | session handle gone → reconnect loop exit | `"stt session handle removed — exiting reconnect loop"` | COVERED (info) |
| R9 | `pipeline/stt.rs:207` | exceeded max reconnects after unexpected disconnect | `"stt exceeded max reconnects after unexpected disconnect"` | COVERED (error) |
| R10 | `pipeline/stt.rs:215` | unexpected disconnect, will sleep + retry | `"stt disconnected unexpectedly; sleeping before reconnect"` | COVERED (warn) |
| R11 | `pipeline/stt_response.rs:34` | soniox frame unparseable → `continue` | `"stt response unparseable — skipping frame"` | COVERED (debug) |
| R12 | `pipeline/stt_response.rs:42` | soniox returned `error_code` → exit | `"stt soniox returned error_code — exiting response processor"` | COVERED (warn) |
| R13 | `pipeline/stt_response.rs:50` | session removed mid-response → exit | `"stt session removed mid-response"` | COVERED (info) |
| R14 | `pipeline/stt_response.rs:86` | ws read error → return None | `"stt websocket read error"` | COVERED (warn) |
| R15 | `pipeline/stt_response.rs:94` | soniox closed cleanly | `"stt soniox closed the connection"` | COVERED (info) |
| R16 | `pipeline/stt_response.rs:109` | soniox embedded error_code in 200 frame | `"stt soniox returned error_code in response"` | COVERED (warn) |
| R17 | `pipeline/stt_response.rs:119` | JSON parse error on STT frame | `"stt response parse error — dropping frame"` | COVERED (warn) |
| R18 | `pipeline/stt_response.rs:140` | token rejected by mode filter → continue | — (normal flow, high volume, not logged by design) | INTENTIONAL NO-LOG |
| R19 | `pipeline/stt_response.rs:143` | end-of-utterance sentinel → continue | — (sentinel handling, not a silent bug path) | INTENTIONAL NO-LOG |
| R20 | `pipeline/stt_response.rs:276` | caption dispatch marker | `"dispatching translated text to caption + tts pipeline"` | COVERED (info) |
| R21 | `pipeline/stt_response.rs:286` | translation produced but no rtmp_manager | `"translation produced but session has no rtmp_manager — captions will not burn in"` | COVERED (warn) |
| R22 | `pipeline/tts.rs:99` | session gone before TTS dispatch | `"tts dispatch aborted: live session removed before ElevenLabs call"` | COVERED (warn — added 2026-04-20) |
| R23 | `pipeline/tts.rs:117` | `BRIVVA_FALLBACK_TO_DEFAULT_VOICE` kill-switch override | `"kill-switch BRIVVA_FALLBACK_TO_DEFAULT_VOICE: forced default voice over selected clone"` | COVERED (warn, has unit test) |
| R24 | `pipeline/tts.rs:155` | ElevenLabs returned empty body → no audio | `"tts dispatch produced no audio — utterance dropped"` | COVERED (warn — added 2026-04-20, was `eprintln!`) |
| R25 | `pipeline/tts.rs:260` | ElevenLabs stream chunk error → break | `"tts elevenlabs stream chunk error — aborting audio read"` | COVERED (warn — added 2026-04-20, was `eprintln!`) |
| R26 | `pipeline/tts.rs:269` | ElevenLabs non-2xx response | `"tts elevenlabs non-2xx response"` | COVERED (warn — added 2026-04-20, was `eprintln!`) |
| R27 | `pipeline/tts.rs:278` | ElevenLabs request transport error | `"tts elevenlabs request error"` | COVERED (warn — added 2026-04-20, was `eprintln!`) |
| R28 | `pipeline/tts.rs:286` | ElevenLabs request timed out | `"tts elevenlabs request timed out"` | COVERED (warn — added 2026-04-20, was `eprintln!`) |
| R29 | `pipeline/tts.rs:298` | TTS produced audio but session has no rtmp_manager | `"tts audio produced but session has no rtmp_manager — dropping"` | COVERED (warn — added 2026-04-20) |
| R30 | `pipeline/tts.rs:313` | mp3→pcm decode failed | `"tts mp3→pcm decode failed — audio dropped"` | COVERED (warn — added 2026-04-20, was `eprintln!`) |
| R31 | `pipeline/tts.rs:324` | session torn down before host notify | `"tts complete but live session gone — host notify skipped"` | COVERED (info — added 2026-04-20) |
| R32 | `pipeline/stt_transport.rs:205` | test-only: listener accept errored | — (test harness only) | TEST-ONLY |
| R33 | `pipeline/stt_transport.rs:206` | test-only: accept_async errored | — (test harness only) | TEST-ONLY |

## Workers silent branches

Workers runs in Cloudflare where structured logs are emitted via
`console.log` / `console.warn` / `console.error`. Grep prefixes follow
the convention `[subsystem] message`.

| # | File:Line | Branch | Expected log (grep key) | Status |
|---|-----------|--------|------------------------|--------|
| W1 | `app.ts:154` | ElevenLabs delete of prior voice failed on upsert | `"[voices] failed to delete prior ElevenLabs voice"` | COVERED |
| W2 | `app.ts:426` | Grip Seller API provision failed → session rollback | `"[sessions] grip seller api provision failed — rolling back session"` | COVERED (added 2026-04-20) |
| W3 | `app.ts:496` | YouTube broadcast create failed → session rollback | `"[sessions] youtube broadcast create failed — rolling back session"` | COVERED (added 2026-04-20) |
| W4 | `app.ts:531` | Defensive D1 error during session-create → rollback | `"[sessions] defensive rollback after session-create exception"` | COVERED (added 2026-04-20) |
| W5 | `app.ts:694` | `target_langs` is not a JSON array | `"parseTargetLangs: stored target_langs is not a JSON array"` | COVERED |
| W6 | `app.ts:702` | `JSON.parse` failed on `target_langs` | `"parseTargetLangs: JSON.parse failed on stored target_langs"` | COVERED |
| W7 | `app.ts:907` | YouTube OAuth callback exception | `"[auth] youtube callback exception"` | COVERED (added 2026-04-20) |
| W8 | `app.ts:950` | Google sign-in callback exception | `"[auth] google sign-in callback exception"` | COVERED (added 2026-04-20) |
| W9 | `stripe-webhook.ts:*` | signature mismatch / rejection | `"[stripe] signature rejected"` / `"[stripe] webhook verified"` | COVERED |
| W10 | `orchestration/app.ts:1045` | Stripe signature rejection (missing) | `"[stripe] signature rejected: missing signature header"` | COVERED |
| W11 | `orchestration/app.ts:1048` | Stripe webhook missing secret (no enforcement) | `"[stripe] webhook received without STRIPE_WEBHOOK_SECRET set"` | COVERED |

---

## Live-session verification run

Once this doc lands, Aziz should run a real 30-min session and grep the
captured log file. Rows currently marked `COVERED` are code-verified
(each branch emits a log call); the live run verifies each branch
actually *fires* during a realistic session shape.

Run the verification:

```bash
# One-shot helper script. Spins up dev-stack, tees stdout+stderr to
# /tmp/brivva-audit-<ts>.log, opens the frontend, and prompts you to run
# through the standard user journey:
#   sign-in → onboard → record voice → create session (2 target langs)
#   → start live → speak for 30 min → end session → verify post-stream
#      summary
bash scripts/post-merge-log-audit.sh
```

After the run, verify each row:

```bash
LOG=/tmp/brivva-audit-<ts>.log
# Example for R22:
grep -c "tts dispatch aborted: live session removed" "$LOG"
# Example for W2:
grep -c "grip seller api provision failed" "$LOG"
```

For rows that return 0 matches: either the branch legitimately did not
fire (the scenario didn't trigger) or the log line is missing. Examine
the session flow — if the scenario *should* have triggered the branch,
add the log call and re-run.

`INTENTIONAL NO-LOG` rows (R18 / R19) are high-volume sentinels in the
per-token hot path; they are NOT §0.5.4 silent-bug paths (they are the
designed-in "skip this token, continue") and adding logs would flood the
stream at ~20 events/sec.

`TEST-ONLY` rows (R32 / R33) are in test harnesses that never run in
production.

---

## Sign-off

- [x] Every silent branch has a designated log line + grep key.
- [x] All prior `eprintln!` calls (which do not reach the structured log
      pipeline) replaced with `tracing::warn!` / `info!`.
- [x] Unit tests still pass (234 in server-rs, 244 in workers).
- [ ] Live-session grep verification — owned by Aziz. Run
      `bash scripts/post-merge-log-audit.sh`, walk the 30-min scenario,
      then tick every row above.

Until the last checkbox is green, §0.5.4 is **code-verified but not
production-verified**. The code-verification closes the gap identified
by P1b (silent paths without any log call); the live run closes the
"wired but not verified" status the P2 reviewer flagged.
