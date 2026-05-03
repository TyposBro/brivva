# TTS Concision

Purpose: keep translated live-commerce speech close enough to realtime without
turning the translation layer into hand-tuned language hacks.

## Current Safety Net

The server currently uses deterministic fallback concision only when translated
audio is already at risk:

- `normal`: send Soniox translation to ElevenLabs unchanged.
- `catch_up`: keep text unchanged and drain translated PCM faster.
- `concise`: shorten translated text before ElevenLabs.
- `hard_recovery`: shorten more aggressively before ElevenLabs and rely on
  whole-segment queue drops only if the live cap is still exceeded.

This is deliberately a safety net, not the desired primary translation style.
It is cheap, local, testable, and preserves critical commerce markers better
than dropping arbitrary PCM bytes. It is still hand-written logic and should not
become the main quality layer.

The deterministic fallback does three things:

- removes common filler or polite phrases for the target language;
- splits text into clauses;
- prefers clauses that contain prices, numbers, discounts, stock, dates, or
  calls to action.

Example:

```text
皆さん本当にありがとうございます。
こちらの商品は今日だけ29,000ウォンで、在庫は50個です。
ぜひ今すぐ購入してください。
```

Can become:

```text
こちらの商品は今日だけ29,000ウォンで 在庫は50個です
```

That keeps the sale facts and drops the greeting/filler.

## Why This Is Not The End State

The fallback scales operationally because activation is driven by measured
backlog and expansion ratio, not by manually deciding "Japanese always shortens"
or "Korean never shortens." But the text operations themselves are hand-authored.
They will never understand every language, every sales phrase, or every brand
tone.

The best version of this system avoids generating long translated text in the
first place.

## Soniox Findings

Soniox real-time translation currently exposes the translation mode as
`one_way` or `two_way` with target/source languages. In the public docs checked
on 2026-05-03, there is no first-class translation field like
`style: concise`, `max_output_tokens`, or `max_chars`.

Useful documented controls:

- `context`: Soniox says context can improve transcription and translation
  accuracy, apply translation preferences, and include custom
  `translation_terms`.
- `general` context accepts arbitrary key-value pairs, and Soniox documents an
  experimental `instructions` key for language behavior.
- `translation_terms` can force important terms, brands, or names to translate
  consistently or remain unchanged.
- `enable_endpoint_detection` finalizes tokens when speech ends.
- `max_endpoint_delay_ms` can reduce endpoint delay, allowed from 500ms to
  3000ms.
- Manual finalization supports `{"type":"finalize"}` and may be called multiple
  times per session, but Soniox warns not to call it too frequently and says
  every few seconds is fine.

Important limitation:

- Translated tokens do not include timestamps. Spoken/original tokens do. So
  source-duration measurement must come from original token timing or local
  audio chunk timing, not translated tokens.

## Recommended Architecture

Use a layered policy:

1. **Ask Soniox for concise live-commerce translation through context.**
   Add session context like:
   - domain: `live commerce`
   - setting: `real-time sales livestream`
   - instructions: `Translate in concise spoken style. Preserve prices,
     product names, stock counts, discounts, dates, and calls to action exactly.
     Avoid filler and excessive politeness.`

2. **Use `translation_terms` for product names, brand names, promo names, and
   units.**
   This is scalable because it is business data, not language-specific code.

3. **Keep endpoint/manual-finalize chunking.**
   Smaller chunks reduce giant TTS requests. Current server already flushes on
   endpoint, punctuation, three sentences, and length. Manual Soniox finalize
   every few seconds is valid if paired with enough trailing silence and not
   called too frequently.

4. **Keep deterministic fallback.**
   If Soniox context does not shorten enough, the current fallback still
   protects the live stream.

5. **If Soniox context is insufficient, add an optional rewrite provider.**
   A low-latency rewrite step can enforce max length better than local rules,
   but it must be behind a feature flag and tested to preserve numbers, product
   names, and legal/product claims exactly.

## Better Than Hand Rules

Best to worst:

1. Soniox native concise/style control, if support confirms it exists.
2. Soniox `context.general.instructions` plus `translation_terms`.
3. Dedicated rewrite provider before TTS.
4. Deterministic fallback filter.
5. Queue overflow/drop.

The current patch implements level 4 because it is reliable today. The next
implementation should try level 2, because it uses a documented Soniox feature
without adding another provider.

## Implementation Proposal

Add context to `SonioxConfig` for translate sessions:

```json
{
  "context": {
    "general": [
      { "key": "domain", "value": "live commerce" },
      { "key": "setting", "value": "real-time sales livestream" },
      {
        "key": "instructions",
        "value": "Translate in concise spoken style. Preserve prices, product names, stock counts, discounts, dates, and calls to action exactly. Avoid filler and excessive politeness."
      }
    ],
    "terms": ["product and brand names from session metadata"],
    "translation_terms": [
      { "source": "BrandName", "target": "BrandName" }
    ]
  },
  "enable_endpoint_detection": true,
  "max_endpoint_delay_ms": 500
}
```

Do not hardcode product names in Rust. Product/brand/promo terms should come
from the session or admin data model.

## Test Plan

- Unit test Soniox translate config includes context instructions only for
  translate sessions.
- Unit test source STT config does not include translation-style instructions.
- Unit test context serializes `translation_terms` without changing existing
  translation mode shape.
- E2E rerun `backlog_catchup_many_outputs` and compare:
  - `tts segment queue overflow`
  - `tts concise live-commerce mode applied`
  - `final_policy="hard_recovery"`
  - per-language `tts_buffered_bytes`

Success is not "zero concision." Success is fewer hard-recovery and queue
overflow events while video and original audio remain at zero drops.
