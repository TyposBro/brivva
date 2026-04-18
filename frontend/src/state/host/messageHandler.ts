import { type Dispatch } from "react";
import { type RoomMessage } from "../../lib/RoomSocket";
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
  return (msg: RoomMessage) => {
    switch (msg.type) {
      case "interim":
        dispatch({ type: "interim", transcript: (msg.transcript as string) ?? "" });
        stopwatch.markInterim();
        break;

      case "final": {
        const uid = String(msg.utteranceId);
        const text = (msg.transcript as string) ?? "";
        dispatch({ type: "final", id: msg.utteranceId as number, transcript: text });
        stopwatch.startTimer(uid, text, getActiveTargetLangs());
        if (typeof msg.sttMs === "number") {
          stopwatch.recordStt(uid, msg.sttMs as number);
        }
        break;
      }

      case "translation": {
        const id = msg.utteranceId as number;
        const text = (msg.text as string) ?? "";
        const targetLang = (msg.targetLang as string) ?? "";
        dispatch({ type: "translation", id, targetLang, text });
        if (typeof msg.translateMs === "number") {
          stopwatch.recordTranslate(String(id), msg.translateMs as number);
        }
        break;
      }

      case "tts_end":
        if (typeof msg.ttsMs === "number") {
          stopwatch.recordTts(String(msg.utteranceId), msg.ttsMs as number);
        }
        break;

      case "video_end":
        // Backend emits this after the per-utterance TTS decode completes —
        // FE uses it as the pipeline-done marker for the latency stopwatch.
        stopwatch.finalize(String(msg.utteranceId), (msg.lipsyncMs as number) ?? 0);
        break;

      case "voice:ready":
        console.log("[HOST] voice cloned:", msg.voiceId);
        dispatch({ type: "voice_ready" });
        break;

      case "error":
        dispatch({ type: "error", message: (msg.message as string) ?? "Unknown error" });
        break;
    }
  };
}
