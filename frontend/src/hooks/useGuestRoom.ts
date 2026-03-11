import { useState, useRef, useEffect } from "react";

const WORKER_URL = import.meta.env.VITE_WORKER_URL ?? "http://localhost:8787";
const WS_URL = WORKER_URL.replace(/^http/, "ws");

export type Lang = "en" | "ja" | "zh";

export type GuestStatus = "idle" | "connecting" | "listening" | "closed" | "error";

export type GuestUtterance = {
  id: number;
  original: string;    // Korean
  translation: string; // in selected lang
};

type TtsEntry = { utteranceId: number; chunks: ArrayBuffer[]; done: boolean };

export function useGuestRoom(roomId: string, lang: Lang | null) {
  const [status, setStatus] = useState<GuestStatus>("idle");
  const [liveTranscript, setLiveTranscript] = useState("");
  const [utterances, setUtterances] = useState<GuestUtterance[]>([]);
  const [error, setError] = useState<string | null>(null);

  const receivingEntryRef = useRef<TtsEntry | null>(null);
  const ttsQueueRef = useRef<TtsEntry[]>([]);
  const isTtsPlayingRef = useRef(false);
  const currentAudioRef = useRef<HTMLAudioElement | null>(null);
  const tryPlayNextRef = useRef<() => void>(() => {});

  function startPlayback(entry: TtsEntry) {
    isTtsPlayingRef.current = true;
    const blob = new Blob(entry.chunks, { type: "audio/mpeg" });
    const url = URL.createObjectURL(blob);
    const audio = new Audio(url);
    currentAudioRef.current = audio;

    const onDone = () => {
      URL.revokeObjectURL(url);
      currentAudioRef.current = null;
      isTtsPlayingRef.current = false;
      ttsQueueRef.current.shift();
      tryPlayNextRef.current();
    };
    audio.onended = onDone;
    audio.onerror = () => { console.error("Audio playback error"); onDone(); };
    audio.play().catch(console.error);
  }

  function tryPlayNext() {
    if (isTtsPlayingRef.current) return;
    const queue = ttsQueueRef.current;
    if (!queue.length) return;
    const entry = queue[0];
    if (!entry.done) return;
    startPlayback(entry);
  }

  tryPlayNextRef.current = tryPlayNext;

  useEffect(() => {
    if (!lang || !roomId) return;

    setStatus("connecting");
    setError(null);
    setUtterances([]);
    setLiveTranscript("");
    ttsQueueRef.current = [];
    receivingEntryRef.current = null;
    isTtsPlayingRef.current = false;

    const ws = new WebSocket(`${WS_URL}/api/room?role=guest&roomId=${encodeURIComponent(roomId)}&lang=${lang}`);
    ws.binaryType = "arraybuffer";

    ws.onmessage = (event) => {
      if (typeof event.data === "string") {
        const msg = JSON.parse(event.data) as { type: string; [k: string]: unknown };

        if (msg.type === "room:joined") {
          setStatus("listening");
        } else if (msg.type === "interim") {
          setLiveTranscript((msg.transcript as string) ?? "");
        } else if (msg.type === "final") {
          const id = msg.utteranceId as number;
          setUtterances((prev) => [
            ...prev,
            { id, original: (msg.transcript as string) ?? "", translation: "" },
          ]);
          setLiveTranscript("");
        } else if (msg.type === "translation") {
          const id = msg.utteranceId as number;
          setUtterances((prev) =>
            prev.map((u) => (u.id === id ? { ...u, translation: (msg.text as string) ?? "" } : u))
          );
        } else if (msg.type === "tts_start") {
          const entry: TtsEntry = { utteranceId: msg.utteranceId as number, chunks: [], done: false };
          receivingEntryRef.current = entry;
          ttsQueueRef.current.push(entry);
        } else if (msg.type === "tts_end") {
          const entry = receivingEntryRef.current;
          if (entry && entry.utteranceId === (msg.utteranceId as number)) {
            entry.done = true;
            receivingEntryRef.current = null;
            tryPlayNextRef.current();
          }
        } else if (msg.type === "room:closed") {
          setStatus("closed");
          ws.close();
        } else if (msg.type === "error") {
          setError((msg.message as string) ?? "Unknown error");
          setStatus("error");
        }
      } else if (event.data instanceof ArrayBuffer) {
        const entry = receivingEntryRef.current;
        if (entry) entry.chunks.push(event.data);
      }
    };

    ws.onclose = () => {
      setStatus((prev) =>
        prev === "listening" || prev === "connecting" ? "closed" : prev
      );
    };
    ws.onerror = () => ws.close();

    return () => ws.close();
  }, [roomId, lang]);

  return { status, liveTranscript, utterances, error };
}
