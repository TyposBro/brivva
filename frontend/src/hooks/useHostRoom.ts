import { useState, useRef, useCallback } from "react";

const WORKER_URL = import.meta.env.VITE_WORKER_URL ?? "http://localhost:8787";
const WS_URL = WORKER_URL.replace(/^http/, "ws");

const SAMPLE_RATE = 16000;
const BUFFER_SIZE = 4096;

export type HostStatus = "idle" | "creating" | "ready" | "recording" | "disconnected";

export type GuestCounts = { en: number; ja: number; zh: number };

export type HostUtterance = { id: number; transcript: string };

export interface UtteranceTiming {
  id: string;
  text: string;
  translateMs: number;
  ttsMs: number;
  totalMs: number;
  overheadMs: number;
  timestamp: number;
  langs: string[];
}

export function useHostRoom() {
  const [status, setStatus] = useState<HostStatus>("idle");
  const [roomId, setRoomId] = useState<string | null>(null);
  const [guestCounts, setGuestCounts] = useState<GuestCounts>({ en: 0, ja: 0, zh: 0 });
  const [liveTranscript, setLiveTranscript] = useState("");
  const [utterances, setUtterances] = useState<HostUtterance[]>([]);
  const [analyser, setAnalyser] = useState<AnalyserNode | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [timings, setTimings] = useState<UtteranceTiming[]>([]);

  const wsRef = useRef<WebSocket | null>(null);
  const audioCtxRef = useRef<AudioContext | null>(null);
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const processorRef = useRef<any>(null);
  const streamRef = useRef<MediaStream | null>(null);

  // Timing tracking refs
  const guestCountsRef = useRef<GuestCounts>({ en: 0, ja: 0, zh: 0 });
  const finalTimestampsRef = useRef<Map<string, { time: number; text: string; langs: string[] }>>(new Map());
  const pendingTranslateRef = useRef<Map<string, number>>(new Map());
  const finalizedRef = useRef<Set<string>>(new Set());

  const stopRecording = useCallback(() => {
    processorRef.current?.disconnect();
    processorRef.current = null;
    streamRef.current?.getTracks().forEach((t) => t.stop());
    streamRef.current = null;
    audioCtxRef.current?.close();
    audioCtxRef.current = null;
    setAnalyser(null);
    setStatus((prev) => (prev === "recording" ? "ready" : prev));
  }, []);

  const createRoom = useCallback(() => {
    setStatus("creating");
    setError(null);
    setRoomId(null);
    setGuestCounts({ en: 0, ja: 0, zh: 0 });
    setUtterances([]);
    setLiveTranscript("");
    setTimings([]);
    guestCountsRef.current = { en: 0, ja: 0, zh: 0 };
    finalTimestampsRef.current.clear();
    pendingTranslateRef.current.clear();
    finalizedRef.current.clear();

    const ws = new WebSocket(`${WS_URL}/api/room?role=host&sourceLang=en`);
    ws.binaryType = "arraybuffer";
    wsRef.current = ws;

    ws.onmessage = (event) => {
      if (typeof event.data !== "string") return;
      const msg = JSON.parse(event.data) as { type: string; [k: string]: unknown };

      if (msg.type === "room:created") {
        setRoomId(msg.roomId as string);
        setStatus("ready");
      } else if (msg.type === "room:guest_count") {
        const counts = msg.counts as GuestCounts;
        setGuestCounts(counts);
        guestCountsRef.current = counts;
      } else if (msg.type === "interim") {
        setLiveTranscript((msg.transcript as string) ?? "");
      } else if (msg.type === "final") {
        const uid = String(msg.utteranceId);
        const text = (msg.transcript as string) ?? "";
        setUtterances((prev) => [
          ...prev,
          { id: msg.utteranceId as number, transcript: text },
        ]);
        setLiveTranscript("");
        // Record pipeline start time for this utterance
        const langs = (["en", "ja", "zh"] as const).filter(
          (l) => guestCountsRef.current[l] > 0
        );
        finalTimestampsRef.current.set(uid, { time: Date.now(), text, langs });
      } else if (msg.type === "translation") {
        // Store translateMs for first language to complete (parallel, so first = fastest)
        const uid = String(msg.utteranceId);
        if (!pendingTranslateRef.current.has(uid)) {
          pendingTranslateRef.current.set(uid, msg.translateMs as number);
        }
      } else if (msg.type === "tts_end") {
        const uid = String(msg.utteranceId);
        // Only finalize once per utterance (first tts_end received)
        if (!finalizedRef.current.has(uid)) {
          finalizedRef.current.add(uid);
          const entry = finalTimestampsRef.current.get(uid);
          if (entry) {
            const totalMs = Date.now() - entry.time;
            const translateMs = pendingTranslateRef.current.get(uid) ?? 0;
            const ttsMs = msg.ttsMs as number;
            setTimings((prev) => [
              {
                id: uid,
                text: entry.text.slice(0, 40),
                translateMs,
                ttsMs,
                totalMs,
                overheadMs: Math.max(0, totalMs - translateMs - ttsMs),
                timestamp: Date.now(),
                langs: entry.langs,
              },
              ...prev.slice(0, 9), // newest first, keep last 10
            ]);
          }
          finalTimestampsRef.current.delete(uid);
          pendingTranslateRef.current.delete(uid);
        }
      } else if (msg.type === "error") {
        setError((msg.message as string) ?? "Unknown error");
      }
    };

    ws.onclose = () => {
      stopRecording();
      setStatus("disconnected");
    };
    ws.onerror = () => ws.close();
  }, [stopRecording]);

  const startRecording = useCallback(async () => {
    const ws = wsRef.current;
    if (!ws || ws.readyState !== WebSocket.OPEN) return;

    const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
    streamRef.current = stream;

    const audioCtx = new AudioContext({ sampleRate: SAMPLE_RATE });
    audioCtxRef.current = audioCtx;

    const source = audioCtx.createMediaStreamSource(stream);
    const analyserNode = audioCtx.createAnalyser();
    analyserNode.fftSize = 512;
    source.connect(analyserNode);
    setAnalyser(analyserNode);

    const processor = audioCtx.createScriptProcessor(BUFFER_SIZE, 1, 1);
    processorRef.current = processor;

    processor.onaudioprocess = (e) => {
      if (ws.readyState !== WebSocket.OPEN) return;
      const float32 = e.inputBuffer.getChannelData(0);
      const int16 = new Int16Array(float32.length);
      for (let i = 0; i < float32.length; i++) {
        int16[i] = Math.max(-32768, Math.min(32767, float32[i] * 32768));
      }
      ws.send(int16.buffer);
    };

    source.connect(processor);
    processor.connect(audioCtx.destination);
    setStatus("recording");
  }, []);

  const closeRoom = useCallback(() => {
    wsRef.current?.send(JSON.stringify({ type: "host:end" }));
    wsRef.current?.close();
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
