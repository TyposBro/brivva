import { Buffer } from "node:buffer";
import type { Bindings } from "../../../core/types";

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
    const base64 = Buffer.from(audioBuffer).toString("base64");

    // Run transcription (original language) + translation (→ English) in parallel
    // whisper-large-v3-turbo: task="translate" always outputs English
    const [transcribeResult, translateResult] = await Promise.all([
      this.env.AI.run("@cf/openai/whisper-large-v3-turbo", {
        audio: base64,
        task: "transcribe",
        language: sourceLang,
        vad_filter: true,
      }),
      this.env.AI.run("@cf/openai/whisper-large-v3-turbo", {
        audio: base64,
        task: "translate",
        language: sourceLang,
        vad_filter: true,
      }),
    ]);

    return {
      transcription: transcribeResult.text?.trim() ?? "",
      translation: translateResult.text?.trim() ?? "",
      sourceLang,
      targetLang,
      durationMs: Date.now() - start,
    };
  }
}
