import { type Dispatch } from "react";
import { type RoomMessage } from "../../lib/RoomSocket";
import { type HostAction, type GuestCounts } from "./reducer";

const GUEST_LANGS = ["en", "ja", "zh"] as const;

type Stopwatch = {
  startTimer: (uid: string, text: string, langs: string[]) => void;
  recordSplit: (uid: string, ms: number) => void;
  finalize: (uid: string, ms: number) => void;
};

export function createMessageHandler(
  dispatch: Dispatch<HostAction>,
  getGuestCounts: () => GuestCounts,
  stopwatch: Stopwatch,
) {
  return (msg: RoomMessage) => {
    switch (msg.type) {
      case "room:created":
        dispatch({ type: "room_created", roomId: msg.roomId as string });
        break;

      case "room:guest_count": {
        const counts = msg.counts as GuestCounts;
        dispatch({ type: "guest_count", counts });
        break;
      }

      case "interim":
        dispatch({ type: "interim", transcript: (msg.transcript as string) ?? "" });
        break;

      case "final": {
        const uid = String(msg.utteranceId);
        const text = (msg.transcript as string) ?? "";
        const activeLangs = GUEST_LANGS.filter((l) => getGuestCounts()[l] > 0);
        dispatch({ type: "final", id: msg.utteranceId as number, transcript: text });
        stopwatch.startTimer(uid, text, activeLangs);
        break;
      }

      case "translation":
        stopwatch.recordSplit(String(msg.utteranceId), msg.translateMs as number);
        break;

      case "tts_end":
        stopwatch.finalize(String(msg.utteranceId), msg.ttsMs as number);
        break;

      case "error":
        dispatch({ type: "error", message: (msg.message as string) ?? "Unknown error" });
        break;
    }
  };
}
