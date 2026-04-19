// Stream timing defaults keyed by (source_lang, target_lang).
//
// Numbers below are product-curated baselines per language pair: more
// agglutinative source languages (Korean) need longer hold buffers because
// the STT/translate pipeline emits later in the utterance; the under-voice
// mix (`host_gain`) sits at 20% so the original is audible without fighting
// the cloned dub.
//
// Source-equal-target streams are passthrough — no translation overlay,
// full original audio, zero added delay.

export interface StreamDefault {
  delay_ms: number;
  host_gain: number;
}

const PASSTHROUGH: StreamDefault = { delay_ms: 0, host_gain: 1.0 };
const BASELINE: StreamDefault = { delay_ms: 2000, host_gain: 0.2 };

/** Sentinel target-lang code for explicit source passthrough. Kept in sync
 *  with `@brivva/contracts/platforms` PASS_LANG_CODE. */
export const PASS_LANG_CODE = "pass";

const TABLE: Record<string, Record<string, StreamDefault>> = {
  ko: {
    zh: { delay_ms: 3000, host_gain: 0.2 },
    ja: { delay_ms: 3000, host_gain: 0.2 },
    en: { delay_ms: 3000, host_gain: 0.2 },
  },
  en: {
    ja: { delay_ms: 2500, host_gain: 0.2 },
    zh: { delay_ms: 2500, host_gain: 0.2 },
    ko: { delay_ms: 2500, host_gain: 0.2 },
  },
  ja: {
    en: { delay_ms: 2500, host_gain: 0.2 },
    ko: { delay_ms: 2500, host_gain: 0.2 },
    zh: { delay_ms: 2500, host_gain: 0.2 },
  },
  zh: {
    en: { delay_ms: 2500, host_gain: 0.2 },
    ja: { delay_ms: 2500, host_gain: 0.2 },
    ko: { delay_ms: 2500, host_gain: 0.2 },
  },
};

export function streamDefault(sourceLang: string, targetLang: string): StreamDefault {
  if (targetLang === PASS_LANG_CODE) return PASSTHROUGH;
  if (sourceLang === targetLang) return PASSTHROUGH;
  return TABLE[sourceLang]?.[targetLang] ?? BASELINE;
}

export function isStreamDefault(
  sourceLang: string,
  targetLang: string,
  candidate: StreamDefault,
): boolean {
  const ref = streamDefault(sourceLang, targetLang);
  return ref.delay_ms === candidate.delay_ms && ref.host_gain === candidate.host_gain;
}
