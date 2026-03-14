import { useReducer, useRef } from "react";
import { AudioPipeline } from "../lib/AudioPipeline";
import { RoomSocket, type RoomMessage } from "../lib/RoomSocket";
import { useTimings, type UtteranceTiming } from "./useTimings";
import { hostReducer, INITIAL_STATE, type GuestCounts } from "./hostReducer";

export type { UtteranceTiming };
export type { HostStatus, GuestCounts, HostUtterance } from "./hostReducer";

const GUEST_LANGS = ["en", "ja", "zh"] as const;

export function useHostRoom() {
  const [state, dispatch] = useReducer(hostReducer, INITIAL_STATE);
  const { timings, recordFinal, recordTranslation, recordTtsEnd, reset: resetTimings } = useTimings();

  const audio = useRef(new AudioPipeline());
  const socket = useRef(new RoomSocket());
  const guestCountsRef = useRef<GuestCounts>({ en: 0, ja: 0, zh: 0 });

  // --- route server messages to state + timings ---

  const handleMessage = (msg: RoomMessage) => {
    switch (msg.type) {
      case "room:created":
        dispatch({ type: "room_created", roomId: msg.roomId as string });
        break;

      case "room:guest_count": {
        const counts = msg.counts as GuestCounts;
        dispatch({ type: "guest_count", counts });
        guestCountsRef.current = counts;
        break;
      }

      case "interim":
        dispatch({ type: "interim", transcript: (msg.transcript as string) ?? "" });
        break;

      case "final": {
        const uid = String(msg.utteranceId);
        const text = (msg.transcript as string) ?? "";
        const activeLangs = GUEST_LANGS.filter((l) => guestCountsRef.current[l] > 0);
        dispatch({ type: "final", id: msg.utteranceId as number, transcript: text });
        recordFinal(uid, text, activeLangs);
        break;
      }

      case "translation":
        recordTranslation(String(msg.utteranceId), msg.translateMs as number);
        break;

      case "tts_end":
        recordTtsEnd(String(msg.utteranceId), msg.ttsMs as number);
        break;

      case "error":
        dispatch({ type: "error", message: (msg.message as string) ?? "Unknown error" });
        break;
    }
  };

  // --- actions ---

  const stopRecording = () => {
    audio.current.stop();
    dispatch({ type: "recording_stopped" });
  };

  const createRoom = () => {
    dispatch({ type: "reset" });
    guestCountsRef.current = { en: 0, ja: 0, zh: 0 };
    resetTimings();
    socket.current.connect(
      { role: "host", sourceLang: "en" },
      { onMessage: handleMessage, onClose: () => { stopRecording(); dispatch({ type: "disconnected" }); } },
    );
  };

  const startRecording = async () => {
    if (!socket.current.isOpen) return;
    const analyser = await audio.current.start((buf) => socket.current.sendAudio(buf));
    dispatch({ type: "recording_started", analyser });
  };

  const closeRoom = () => {
    socket.current.sendJson({ type: "host:end" });
    socket.current.close();
    stopRecording();
  };

  return {
    ...state, timings,
    createRoom, startRecording, stopRecording, closeRoom,
  };
}
