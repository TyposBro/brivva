// Stream timing defaults keyed by (source_lang, target_lang).
//
// Translated streams hold host media long enough for STT, translation, and
// TTS to land before playback. Server also enforces this as a minimum, so
// frontend hints cannot make production streams race the TTS pipeline.
//
// Source-equal-target streams are passthrough — no translation overlay,
// full original audio, zero added delay.

export interface StreamDefault {
  delay_ms: number;
  host_gain: number;
}

const PASSTHROUGH: StreamDefault = { delay_ms: 0, host_gain: 1.0 };
const TRANSLATED: StreamDefault = { delay_ms: 4000, host_gain: 0.03 };

/** Sentinel target-lang code for explicit source passthrough. Kept in sync
 *  with `@brivva/contracts/platforms` PASS_LANG_CODE. */
export const PASS_LANG_CODE = "pass";

const TABLE: Record<string, Record<string, StreamDefault>> = {
  ko: {
    zh: TRANSLATED,
    ja: TRANSLATED,
    en: TRANSLATED,
    th: TRANSLATED,
    vi: TRANSLATED,
    id: TRANSLATED,
  },
  en: {
    ja: TRANSLATED,
    zh: TRANSLATED,
    ko: TRANSLATED,
  },
  ja: {
    en: TRANSLATED,
    ko: TRANSLATED,
    zh: TRANSLATED,
  },
  zh: {
    en: TRANSLATED,
    ja: TRANSLATED,
    ko: TRANSLATED,
  },
};

export function streamDefault(sourceLang: string, targetLang: string): StreamDefault {
  if (targetLang === PASS_LANG_CODE) return PASSTHROUGH;
  if (sourceLang === targetLang) return PASSTHROUGH;
  return TABLE[sourceLang]?.[targetLang] ?? TRANSLATED;
}

export function isStreamDefault(
  sourceLang: string,
  targetLang: string,
  candidate: StreamDefault,
): boolean {
  const ref = streamDefault(sourceLang, targetLang);
  return ref.delay_ms === candidate.delay_ms && ref.host_gain === candidate.host_gain;
}
