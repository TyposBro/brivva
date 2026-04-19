import { type Dispatch } from "react";
import { HostServerMessageSchema } from "@brivva/contracts/ws";
import { type HostAction } from "./reducer";

type Stopwatch = {
  startTimer: (uid: string, text: string, langs: string[]) => void;
  markInterim: () => void;
  recordStt: (uid: string, ms: number) => void;
  recordTranslate: (uid: string, ms: number) => void;
  recordTts: (uid: string, ms: number) => void;
  finalize: (uid: string, lipsyncMs: number) => void;
};

// Backend → frontend message contract. All guest-facing variants are gone.
// Target-specific events (translation, tts_end) are tagged with targetLang so
// the host UI can attribute timings and text per target stream.
export function createMessageHandler(
  dispatch: Dispatch<HostAction>,
  getActiveTargetLangs: () => string[],
  stopwatch: Stopwatch,
) {
  return (msg: unknown) => {
    const parsed = HostServerMessageSchema.safeParse(msg);
    if (!parsed.success) {
      return;
    }

    const message = parsed.data;
    switch (message.type) {
      case "interim":
        dispatch({ type: "interim", transcript: message.transcript });
        stopwatch.markInterim();
        break;

      case "final": {
        const uid = String(message.utteranceId);
        const text = message.transcript;
        dispatch({ type: "final", id: message.utteranceId, transcript: text });
        stopwatch.startTimer(uid, text, getActiveTargetLangs());
        if (typeof message.sttMs === "number") {
          stopwatch.recordStt(uid, message.sttMs);
        }
        break;
      }

      case "translation": {
        const id = message.utteranceId;
        const text = message.text;
        const targetLang = message.targetLang;
        dispatch({ type: "translation", id, targetLang, text });
        if (typeof message.translateMs === "number") {
          stopwatch.recordTranslate(String(id), message.translateMs);
        }
        break;
      }

      case "tts_end":
        if (typeof message.ttsMs === "number") {
          stopwatch.recordTts(String(message.utteranceId), message.ttsMs);
        }
        break;

      case "video_end":
        // Backend emits this after the per-utterance TTS decode completes —
        // FE uses it as the pipeline-done marker for the latency stopwatch.
        stopwatch.finalize(String(message.utteranceId), message.lipsyncMs ?? 0);
        break;

      case "error":
        dispatch({ type: "error", message: message.message });
        break;
    }
  };
}
