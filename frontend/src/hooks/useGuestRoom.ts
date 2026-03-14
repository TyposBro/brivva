import { useState, useRef, useEffect } from "react";
import { RoomSocket } from "../lib/RoomSocket";
import { TtsPlayer } from "../lib/TtsPlayer";

export type Lang = "en" | "ja" | "zh";
export type GuestStatus = "idle" | "connecting" | "listening" | "closed" | "error";
export type GuestUtterance = {
  id: number;
  original: string;
  translation: string;
};

export function useGuestRoom(roomId: string, lang: Lang | null) {
  const [status, setStatus] = useState<GuestStatus>("idle");
  const [liveTranscript, setLiveTranscript] = useState("");
  const [utterances, setUtterances] = useState<GuestUtterance[]>([]);
  const [error, setError] = useState<string | null>(null);

  const socket = useRef(new RoomSocket());
  const player = useRef(new TtsPlayer());

  useEffect(() => {
    if (!lang || !roomId) return;

    setStatus("connecting");
    setError(null);
    setUtterances([]);
    setLiveTranscript("");
    player.current.reset();

    socket.current.connect(
      { role: "guest", roomId, lang },
      {
        onMessage: (msg) => {
          switch (msg.type) {
            case "room:joined":
              setStatus("listening");
              break;

            case "interim":
              setLiveTranscript((msg.transcript as string) ?? "");
              break;

            case "final":
              setUtterances((prev) => [
                ...prev,
                { id: msg.utteranceId as number, original: (msg.transcript as string) ?? "", translation: "" },
              ]);
              setLiveTranscript("");
              break;

            case "translation": {
              const id = msg.utteranceId as number;
              setUtterances((prev) =>
                prev.map((u) => (u.id === id ? { ...u, translation: (msg.text as string) ?? "" } : u)),
              );
              break;
            }

            case "tts_start":
              player.current.startReceiving(msg.utteranceId as number);
              break;

            case "tts_end":
              player.current.finishReceiving(msg.utteranceId as number);
              break;

            case "room:closed":
              setStatus("closed");
              socket.current.close();
              break;

            case "error":
              setError((msg.message as string) ?? "Unknown error");
              setStatus("error");
              break;
          }
        },
        onBinary: (data) => player.current.addChunk(data),
        onClose: () => {
          setStatus((prev) => (prev === "listening" || prev === "connecting" ? "closed" : prev));
        },
      },
    );

    return () => socket.current.close();
  }, [roomId, lang]);

  return { status, liveTranscript, utterances, error };
}
