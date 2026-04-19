# Voice Clone Language Hints — Implementation Notes

Context: the April 2026 Indian-accent regression, where Korean host audio
cloned via ElevenLabs IVC produced TTS output with an unmistakably Indian
English accent. Root cause was a combination of a short host sample and
IVC defaulting to an English-biased speaker profile. These notes explain
what we now send at clone time and where the real levers live.

## What Workers send to `POST /v1/voices/add`

When the frontend supplies `source_lang` on `/api/voices` or
`/api/sessions/:id/voice`, the Worker forwards it as a `labels` form field:

```json
{"language": "ko"}
```

See `workers/src/features/voices/elevenlabs-client.ts`.

`labels` is a free-form metadata map on the IVC endpoint; ElevenLabs does
**not** expose a first-class `language` parameter on `/v1/voices/add` for
the IVC model. The label rides along on the voice entry and is visible in
`/v1/voices` — downstream services (our Fargate TTS layer) can read it
and use it to pick a language-appropriate TTS `model_id` at synthesis
time.

## Where accent quality really comes from

Three variables, in order of impact:

1. **Sample duration.** Workers reject < 30s and cap at 3min. Best results
   at 2min+. Enforced in `workers/src/shared/audio/wav-duration.ts`.
2. **Sample language matching the speaker's native language.** A Korean
   host reading Korean clones cleanly. Same host reading English will
   imprint English phonotactics on the clone, which is what bit us in April.
3. **`model_id` at TTS time.** `eleven_multilingual_v2` handles Korean
   source well; `eleven_turbo_v2` is English-biased. Model selection lives
   in `server-rs/src/features/broadcast/data/pipeline/tts.rs`, not in
   Workers.

The Workers-side label is a hint for #3; it is not a substitute for #1
and #2.

## Fallback if the label stops working

If ElevenLabs removes `labels` support or changes its semantics, the
downstream model-id picker is the fix-at-the-source lever. We keep the
label wire-up because it is the documented escape hatch and costs nothing
when correct, but the real test-in-prod of accent quality is the TTS
`model_id` choice at synthesis.

The `BRIVVA_FALLBACK_TO_DEFAULT_VOICE=1` kill-switch (see
`docs/runbook.md`) remains the incident-time lever if clones are producing
garbage mid-show.
