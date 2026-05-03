# Rust Server Stress Test

Purpose: prove `server-rs` live media path works outside ideal local conditions.
This file is intentionally an index so future agents can load only the relevant
section instead of reading one large doc.

## Start Here

- [Commands](rust-stress-test/commands.md): how to run full or single scenarios.
- [Signals](rust-stress-test/signals.md): what log lines mean healthy vs bad.
- [Live Failure Policy](rust-stress-test/live_failure_policy.md): product/media
  behavior for STT, TTS, FFmpeg, browser codec, and language expansion issues.
- [TTS Concision](rust-stress-test/tts_concision.md): how translated text is
  shortened today, why it is a fallback, and Soniox-backed options to explore.
- [Scenarios](rust-stress-test/scenarios.md): scenario-by-scenario stress plan.
- [Future Automation](rust-stress-test/future_automation.md): repeatable chaos
  flags still worth adding.

## Most Common Command

```bash
infisical run --env=dev --path=/ -- \
  ./scripts/run-rust-stress-tests.sh --only backlog_catchup_many_outputs
```

The runner writes logs under `tmp/rust-stress-logs/`.
