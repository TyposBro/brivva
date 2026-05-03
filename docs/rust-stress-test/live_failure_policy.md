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
- Treat translated TTS as a continuous live speech lane, not a strict
  one-to-one replacement for each original utterance.
- Keep TTS playback at normal speed when translated audio backlog is low.

Expected user experience: translated audio is smooth and slightly delayed.

## Translated Speech Timing Model

Translated audio does not have to fit exactly inside the source utterance that
created it. Natural hosts pause, repeat, breathe, show products, and wait for
chat. A good live-commerce dub should use those gaps to keep a continuous
translated speech lane roughly behind the host, not squeeze every translated
sentence into the exact original duration.

Target behavior:

- translated audio should normally trail the host by about 1s;
- short spikes can use the live backlog window;
- translated speech should remain intelligible before it tries to be perfectly
  synced;
- the source/pass stream remains the truth for timing and fallback.

Wrong behavior:

- speeding TTS so much that viewers cannot understand it;
- matching each translated segment to the source duration at any cost;
- chopping translated PCM in the middle of words;
- replaying very stale product/pricing information just because it was
  technically preserved.

Current implementation implication:

- TTS catch-up playback has a conservative cap. It may run slightly faster, but
  it must not become unintelligible. If backlog still grows, the system should
  use upstream concise translation, sentence chunking, and whole-segment
  recovery instead of extreme playback speed.

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
- Drain translated audio slightly faster until it catches up.
- Keep speed bounded for intelligibility; for Japanese, aggressive speed-up was
  observed to make TTS effectively inaudible/unusable.
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
- Do not use extreme playback speed as the primary recovery mechanism.
- Log language, utterance/chunk id, text length, duration, and reason.

Implemented server behavior: RTMP TTS queue stores `TtsSegment` entries, catches
up by consuming translated PCM faster when backlog grows, and only drops whole
segments when the hard live cap is exceeded.

Current catch-up speed policy:

- mild translated backlog: `1.15x`;
- stronger translated backlog: `1.3x`;
- critical translated backlog: `1.3x`;
- above that, prefer concise/hard-recovery policy over faster playback.

Implementation caution: translated audio is PCM `s16le`, so catch-up drain must
consume only even byte counts. Draining an odd number of bytes shifts the queue
by half a sample and turns later TTS into harsh noise rather than human speech.

## Translated Audio Too Long

Observed in `backlog_catchup_many_outputs` on 2026-05-03: the media path stayed
healthy, but Japanese TTS repeatedly hit the 15s live cap. That means the issue
was no longer FFmpeg, RTMP, or Rust queue safety; the translated Japanese audio
was longer than live playback could drain.

Follow-up e2e on 2026-05-03 after adding expansion observability:

- Test: `backlog_catchup_many_outputs`
- Result: passed in 180.58s.
- FFmpeg stayed realtime near the end: `speed=1x`, `drop_frames=0`.
- Video stale drops: `0`.
- Host/original audio drops: `0`.
- `tts concise live-commerce mode applied`: 50 times.
- `final_policy="catch_up"`: 13 times.
- `final_policy="concise"`: 61 times.
- `final_policy="hard_recovery"`: 6 times.
- `tts segment queue overflow`: 25 times, all Japanese.

Conclusion: the queue and media transport are healthy enough for this scenario,
but Japanese text/audio expansion still exceeds the live window. The first
concise implementation mostly removed whitespace and capped long paragraphs;
many real Japanese chunks only shrank by one character, so it did not materially
reduce ElevenLabs output duration.

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

Implemented patch path:

1. Per-segment timing and policy metadata is logged and stored with queued TTS
   segments.
2. Catch-up playback drains translated PCM faster while backlog is moderate.
3. Whole-segment TTS drops are used at the hard cap instead of cutting PCM in
   the middle of a word.
4. Concise mode now performs deterministic live-commerce shortening before
   ElevenLabs:
   - remove common filler/polite phrases by target language;
   - prefer clauses containing prices, numbers, discounts, stock, dates, or CTA;
   - apply tighter caps during hard recovery than normal concise mode.

Remaining recommended improvement:

- Move concision upstream into translation style when possible. Soniox
  translation currently returns final translated text; if it supports style or
  glossary instructions, request "short live-commerce translation" before the
  text reaches TTS. If Soniox cannot do this, add a dedicated low-latency rewrite
  step that preserves prices, product names, stock counts, discounts, dates, and
  CTA exactly, then feeds the shorter text to ElevenLabs.
- Keep translated audio as continuous speech around the live edge. The goal is
  not exact utterance-duration matching; the goal is understandable translated
  commerce speech with low enough delay that price, stock, and CTA are still
  current.

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

Viewer scenarios:

- Normal product pitch: host says "This serum is 29,000 won today, only 50
  left" in 4s. If translated TTS is about 4.5-4.8s, play normally. The viewer
  hears a small natural delay.
- Japanese politeness expansion: host says a fast 5s Korean sales line, but
  Japanese TTS becomes 9s because it adds polite/explanatory phrasing. This is
  `1.8x`, so switch to concise live-commerce mode. Preserve price, product,
  discount, stock, deadline, and CTA; cut filler and excessive politeness.
- Chinese compact text but slow voice: host speaks 6s and Chinese text is short,
  but the selected TTS voice takes 8s. This is `1.33x`, so first use playback
  catch-up instead of rewriting. The problem may be voice pacing, not text
  verbosity.
- Korean tone risk: host speaks English for 5s and Korean TTS takes 6.5s. This
  is `1.3x`, so catch up first. Do not aggressively shorten Korean by default,
  because removing honorific tone too early can sound rude.
- Host ramble with no pause: host talks for 40s continuously. Manual Soniox
  finalize, sentence grouping, and max-length flush should turn the ramble into
  live chunks. Each chunk gets measured; only long/backlogged chunks enter
  concise mode.
- Fast-changing sale info: host says "29,000 won", then 10s later says "flash
  deal, now 19,000 won." A 20s delayed translation is harmful because viewers
  hear stale pricing. This is why hard recovery exists: complete-but-late can be
  worse than missing-but-live.

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
