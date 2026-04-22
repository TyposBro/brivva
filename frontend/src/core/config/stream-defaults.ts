// Stream timing defaults keyed by (source_lang, target_lang).
//
// Numbers below are product-curated baselines per language pair: more
// agglutinative source languages (Korean) still hold slightly longer
// because the STT/translate pipeline emits later in the utterance, but
// the overall range is tight (500–1000 ms) so the dub tracks the host
// closely. The under-voice mix (`host_gain`) sits at 3% so the original
// is barely audible and does not fight the cloned dub.
//
// Source-equal-target streams are passthrough — no translation overlay,
// full original audio, zero added delay.

export interface StreamDefault {
  delay_ms: number;
  host_gain: number;
}

const PASSTHROUGH: StreamDefault = { delay_ms: 0, host_gain: 1.0 };
const BASELINE: StreamDefault = { delay_ms: 750, host_gain: 0.03 };

/** Sentinel target-lang code for explicit source passthrough. Kept in sync
 *  with `@brivva/contracts/platforms` PASS_LANG_CODE. */
export const PASS_LANG_CODE = "pass";

const TABLE: Record<string, Record<string, StreamDefault>> = {
  ko: {
    zh: { delay_ms: 1000, host_gain: 0.03 },
    ja: { delay_ms: 1000, host_gain: 0.03 },
    en: { delay_ms: 1000, host_gain: 0.03 },
    th: { delay_ms: 750, host_gain: 0.03 },
    vi: { delay_ms: 750, host_gain: 0.03 },
    id: { delay_ms: 750, host_gain: 0.03 },
  },
  en: {
    ja: { delay_ms: 750, host_gain: 0.03 },
    zh: { delay_ms: 750, host_gain: 0.03 },
    ko: { delay_ms: 750, host_gain: 0.03 },
  },
  ja: {
    en: { delay_ms: 750, host_gain: 0.03 },
    ko: { delay_ms: 750, host_gain: 0.03 },
    zh: { delay_ms: 750, host_gain: 0.03 },
  },
  zh: {
    en: { delay_ms: 750, host_gain: 0.03 },
    ja: { delay_ms: 750, host_gain: 0.03 },
    ko: { delay_ms: 750, host_gain: 0.03 },
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
