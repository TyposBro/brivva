# AGENT_PROGRESS

## Current task
Add Yuna and Gitae instant-clone voice presets available for every target language.

## Checklist
- [x] Read current voice preset flow across frontend, Workers contracts, and server-rs TTS.
- [x] Extend shared voice preset contracts to include `yuna` and `gitae`.
- [x] Add frontend picker options alongside default female/male voices.
- [x] Add server-rs preset enum + ElevenLabs voice IDs.
- [x] Route Yuna/Gitae through cloned TTS path with target-language steering.
- [x] Add/update tests.
- [x] Run validation.
- [ ] Commit and push.

## Voice IDs
- Yuna instant clone: `TGckuO5QVcA50jWnQibB`
- Gitae instant clone: `K0oVfsHF8uZXht1iFdGi`

## Completed work
- Added `yuna` and `gitae` to contract `VoicePresetSchema`, OpenAPI, generated frontend/Rust contracts, Workers DB type, and frontend API type.
- Added Yuna/Gitae options to `VoicePresetPicker`.
- Let built-in presets show in setup even without `session.voice_id`.
- Added `VoicePreset::Yuna` and `VoicePreset::Gitae` on server-rs.
- Mapped presets to ElevenLabs voice IDs.
- Routed built-in instant clones as cloned TTS voices with `language_code` set to the target language so they work across every target language.
- Preserved user cloned voice behavior: user clones still use enrollment-language steering when known.

## Tests run
- `bun run contracts:generate` ✅
- `bun run --cwd frontend test voice-preset-picker session-setup-page` ✅
- `bun run typecheck:workers` ✅
- `bun run typecheck:frontend` ✅
- `cargo test -p server-rs tts --lib` ✅
- `cargo test -p server-rs --test tts_cross_lang_clone` ✅
- `cargo test -p server-rs` ✅

## Commits
- Pending.

## Blockers
- None.

## Next action
Commit and push.
