import { useState, useRef, useCallback } from "react";
import { AudioPipeline } from "./AudioPipeline";
import { RoomSocket, type RoomMessage } from "./RoomSocket";
import { useTimings, type UtteranceTiming } from "./useTimings";

export type { UtteranceTiming };

export type HostStatus =
  | "idle"
  | "creating"
  | "ready"
  | "recording"
  | "disconnected";
export type GuestCounts = { en: number; ja: number; zh: number };
export type HostUtterance = { id: number; transcript: string };

const GUEST_LANGS = ["en", "ja", "zh"] as const;

export function useHostRoom() {
  const [status, setStatus] = useState<HostStatus>("idle");
  const [roomId, setRoomId] = useState<string | null>(null);
  const [guestCounts, setGuestCounts] = useState<GuestCounts>({
    en: 0,
    ja: 0,
    zh: 0,
  });
  const [liveTranscript, setLiveTranscript] = useState("");
  const [utterances, setUtterances] = useState<HostUtterance[]>([]);
  const [analyser, setAnalyser] = useState<AnalyserNode | null>(null);
  const [error, setError] = useState<string | null>(null);

  const {
    timings,
    recordFinal,
    recordTranslation,
    recordTtsEnd,
    reset: resetTimings,
  } = useTimings();

  const audio = useRef(new AudioPipeline());
  const socket = useRef(new RoomSocket());
  // Ref so the WS message callback always reads the latest counts without stale closure
  const guestCountsRef = useRef<GuestCounts>({ en: 0, ja: 0, zh: 0 });

  const stopRecording = useCallback(() => {
    audio.current.stop();
    setAnalyser(null);
    setStatus((prev) => (prev === "recording" ? "ready" : prev));
  }, []);

  const handleMessage = useCallback(
    (msg: RoomMessage) => {
      switch (msg.type) {
        case "room:created":
          setRoomId(msg.roomId as string);
          setStatus("ready");
          break;

        case "room:guest_count": {
          const counts = msg.counts as GuestCounts;
          setGuestCounts(counts);
          guestCountsRef.current = counts;
          break;
        }

        case "interim":
          setLiveTranscript((msg.transcript as string) ?? "");
          break;

        case "final": {
          const uid = String(msg.utteranceId);
          const text = (msg.transcript as string) ?? "";
          const activeLangs = GUEST_LANGS.filter(
            (l) => guestCountsRef.current[l] > 0
          );
          setUtterances((prev) => [
            ...prev,
            { id: msg.utteranceId as number, transcript: text },
          ]);
          setLiveTranscript("");
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
          setError((msg.message as string) ?? "Unknown error");
          break;
      }
    },
    [recordFinal, recordTranslation, recordTtsEnd]
  );

  const handleClose = useCallback(() => {
    stopRecording();
    setStatus("disconnected");
  }, [stopRecording]);

  const createRoom = useCallback(() => {
    setStatus("creating");
    setError(null);
    setRoomId(null);
    setGuestCounts({ en: 0, ja: 0, zh: 0 });
    setUtterances([]);
    setLiveTranscript("");
    guestCountsRef.current = { en: 0, ja: 0, zh: 0 };
    resetTimings();

    socket.current.connect(
      { role: "host", sourceLang: "en" },
      { onMessage: handleMessage, onClose: handleClose }
    );
  }, [handleMessage, handleClose, resetTimings]);

  const startRecording = useCallback(async () => {
    if (!socket.current.isOpen) return;
    const analyserNode = await audio.current.start((buf) =>
      socket.current.sendAudio(buf)
    );
    setAnalyser(analyserNode);
    setStatus("recording");
  }, []);

  const closeRoom = useCallback(() => {
    socket.current.sendJson({ type: "host:end" });
    socket.current.close();
    stopRecording();
  }, [stopRecording]);

  return {
    status,
    roomId,
    guestCounts,
    liveTranscript,
    utterances,
    analyser,
    error,
    timings,
    createRoom,
    startRecording,
    stopRecording,
    closeRoom,
  };
}
