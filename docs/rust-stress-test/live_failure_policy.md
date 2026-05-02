# Live Failure Policy

Brivva is a live-commerce streamer. Correct behavior is not "never drop
anything"; correct behavior is "keep the show live, preserve meaning, and make
every degradation visible in logs." A translated sentence arriving one minute
late can be worse than a missing sentence because price, inventory, and CTA may
already have changed.

## Normal Speech

Host speaks in short product sentences.

Policy:

- Let Soniox endpoint detection finalize naturally.
- Flush translated text after punctuation.
- Send up to three short sentences per TTS request.
- Keep TTS playback at normal speed when translated audio backlog is under 2s.

Expected user experience: translated audio is smooth and slightly delayed.

## Long Ramble / No Pause

Host talks continuously, reads a long paragraph, or counts from 1 to 100.

Risk: Soniox may wait for a pause and hold one giant utterance. One giant TTS
request returns late, then the translated stream falls behind.

Policy:

- Send Soniox manual finalize every `BRIVVA_STT_FORCE_FINALIZE_MS` of active
  audio. Default: `3000`.
- Flush translated text after three sentence boundaries.
- Force-flush translated text by length at roughly 100 chars.

Expected user experience: translation arrives in live chunks. It may sound a
little less literary, but it does not freeze for a full monologue.

## Slight TTS Backlog

TTS is 2-5s behind because ElevenLabs returned a chunk late.

Policy:

- Keep host audio/video on time.
- Drain translated audio faster until it catches up.
- Log `tts_buffered_bytes`, `tts_buffered_segments`, `tts_playback_speed`,
  and `tts_catchup_active`.

Expected user experience: translated voice gets slightly faster for a short
period, then returns to normal.

## Severe TTS Backlog

TTS is more than 10-15s behind.

Risk: even if every translated sentence is preserved, viewers hear stale product
information.

Policy:

- Prefer whole translated sentence/chunk drops over raw PCM byte drops.
- Never cut a translated word mid-audio.
- Log language, utterance/chunk id, text length, duration, and reason.

Implemented server behavior: RTMP TTS queue stores `TtsSegment` entries, catches
up by consuming translated PCM faster when backlog grows, and only drops whole
segments when the hard live cap is exceeded.

## Translated Audio Too Long

Observed in `backlog_catchup_many_outputs` on 2026-05-03: the media path stayed
healthy, but Japanese TTS repeatedly hit the 15s live cap. That means the issue
was no longer FFmpeg, RTMP, or Rust queue safety; the translated Japanese audio
was longer than live playback could drain.

Risk: a language can be technically "working" while the viewer hears old
product information because translated speech is too verbose or synthesized too
slowly.

Recommended language-agnostic policy:

- Track each translated segment's source audio duration, translated text length,
  TTS PCM duration, and expansion ratio.
- If `tts_duration / source_duration` is healthy, play normally.
- If expansion is high, request concise live-commerce translation before TTS.
- If backlog still grows, compress TTS playback up to the configured max.
- If backlog still exceeds the hard live cap, drop whole translated segments.

Do not hand-tune every language pair first. Start with dynamic ratios:

- `normal`: TTS duration <= 1.2x source speech duration.
- `watch`: 1.2x-1.5x, allow catch-up speed.
- `degraded`: >1.5x or backlog >5s, request shorter translation style.
- `emergency`: backlog >10-15s, drop whole translated segments.

Recommended patch path:

1. Add per-segment timing metadata:
   `source_duration_ms`, `tts_duration_ms`, `expansion_ratio`, `text_chars`.
2. Add concise translation mode when expansion/backlog is high.
3. Add provider-level TTS speed controls if ElevenLabs supports stable
   per-request speaking-rate control for the selected model/voice.
4. Keep playback catch-up and whole-segment dropping as safety nets.

Language implications:

- Japanese often expands in politeness and explanatory phrasing. Concise mode
  should preserve price, product, quantity, discount, deadline, and CTA, but
  cut filler and repeated honorific phrasing. Too much compression may sound
  blunt, but that is usually better than stale pricing.
- Chinese can be compact in text, but TTS pacing and punctuation can still
  create long audio. Concise mode should preserve numbers and product claims
  exactly. Risk is over-compression making sales tone too abrupt.
- Korean source streams are often already concise for Korean commerce. Korean
  target TTS may still need pacing controls, but aggressive shortening can make
  honorifics sound rude. Use ratio/backlog triggers rather than always-on
  shortening.
- English can be wordier than CJK for some product details. Dynamic expansion
  ratio still applies; do not assume English is always safe.

Flexible default:

- Keep one global policy driven by measured expansion ratio and backlog.
- Use per-language overrides only for voice/style prompts and regulatory
  wording, not for core queue behavior.
- Prefer preserving exact numbers, prices, discounts, product names, stock
  counts, dates, and CTA over preserving every filler phrase.

## ElevenLabs Slow Or Down

Risk: TTS request times out or returns non-2xx.

Policy:

- Drop only that TTS request.
- Keep source/original stream alive.
- Keep translated subtitles if Soniox translation succeeded.
- Log `utterance_id`, language, deadline, and provider error.

Expected user experience: translated voice may disappear briefly; original show
continues.

## Soniox Slow Or Down

Risk: no STT, no subtitles, no translated TTS.

Policy:

- Keep original stream alive.
- Reconnect Soniox up to configured retries.
- Log provider error and reconnect count.

Expected user experience: original show continues; translation temporarily stops.

## FFmpeg Or RTMP Backpressure

Risk: encoder or platform upload stalls.

Policy:

- Keep audio and video together inside the short lag window.
- Catch up by draining faster when FFmpeg accepts writes again.
- Drop only after max lag.
- For video, resume on IDR/keyframe when stale packets were dropped.
- Log exact dropped counters.

Expected user experience: short stalls recover; severe stalls may create a
visible jump instead of endless buffering.

## Too Many Outputs

Risk: GPU/CPU/network cannot handle every language/platform encoder.

Policy:

- Detect `speed < 0.95` and growing buffers.
- Prefer lowering resolution/FPS or disabling lower-priority outputs over
  letting every stream degrade.
- Long-term: use per-language encoded fanout so one encode feeds multiple RTMP
  publishers for the same language.

Expected user experience: high-priority streams stay healthy first.

## Browser Cannot Provide H.264

Risk: browser/device sends unsupported codec. RTMP platforms require H.264.

Policy:

- Server accepts H.264 only.
- Frontend requests H.264 preferred codec.
- If unavailable, fail before going live with a clear browser/device error.

Expected user experience: clear early failure instead of broken livestream.
