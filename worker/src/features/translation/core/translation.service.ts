import type { Bindings } from "../../../core/types";

// FLORES-200 codes for NLLB translation model
const NLLB_LANG_MAP: Record<string, string> = {
  ko: "kor_Hang",
  en: "eng_Latn",
  ja: "jpn_Jpan",
  zh: "zho_Hans",
  es: "spa_Latn",
  fr: "fra_Latn",
  de: "deu_Latn",
  pt: "por_Latn",
  ar: "arb_Arab",
  ru: "rus_Cyrl",
};

export type TranslationResult = {
  transcription: string;
  translation: string;
  sourceLang: string;
  targetLang: string;
  durationMs: number;
};

export class TranslationService {
  constructor(private env: Bindings) {}

  async process(
    audioBuffer: ArrayBuffer,
    sourceLang: string,
    targetLang: string
  ): Promise<TranslationResult> {
    const start = Date.now();

    // 1. STT — Whisper via Cloudflare Workers AI
    const audioArray = [...new Uint8Array(audioBuffer)];
    const sttResult = await this.env.AI.run("@cf/openai/whisper", {
      audio: audioArray,
    });
    const transcription = sttResult.text?.trim() ?? "";

    if (!transcription) {
      return { transcription: "", translation: "", sourceLang, targetLang, durationMs: Date.now() - start };
    }

    // 2. Translate — NLLB via Cloudflare Workers AI
    const nllbSource = NLLB_LANG_MAP[sourceLang] ?? "kor_Hang";
    const nllbTarget = NLLB_LANG_MAP[targetLang] ?? "eng_Latn";

    const translationResult = await this.env.AI.run(
      "@cf/facebook/nllb-200-distilled-600M",
      {
        text: transcription,
        source_lang: nllbSource,
        target_lang: nllbTarget,
      }
    );

    return {
      transcription,
      translation: translationResult.translated_text ?? "",
      sourceLang,
      targetLang,
      durationMs: Date.now() - start,
    };
  }
}
