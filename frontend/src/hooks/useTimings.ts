import { useState, useRef } from "react";

const MAX_TIMINGS = 10;

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

export function useTimings() {
  const [timings, setTimings] = useState<UtteranceTiming[]>([]);

  const startTimesRef = useRef(new Map<string, { time: number; text: string; langs: string[] }>());
  const translateMsRef = useRef(new Map<string, number>());
  const finalizedRef = useRef(new Set<string>());

  const recordFinal = (uid: string, text: string, langs: string[]) => {
    startTimesRef.current.set(uid, { time: Date.now(), text, langs });
  };

  const recordTranslation = (uid: string, translateMs: number) => {
    if (!translateMsRef.current.has(uid)) translateMsRef.current.set(uid, translateMs);
  };

  const recordTtsEnd = (uid: string, ttsMs: number) => {
    if (finalizedRef.current.has(uid)) return;
    const entry = startTimesRef.current.get(uid);
    if (!entry) return;

    finalizedRef.current.add(uid);
    const totalMs = Date.now() - entry.time;
    const translateMs = translateMsRef.current.get(uid) ?? 0;

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
      ...prev.slice(0, MAX_TIMINGS - 1),
    ]);

    startTimesRef.current.delete(uid);
    translateMsRef.current.delete(uid);
  };

  const reset = () => {
    setTimings([]);
    startTimesRef.current.clear();
    translateMsRef.current.clear();
    finalizedRef.current.clear();
  };

  return { timings, recordFinal, recordTranslation, recordTtsEnd, reset };
}
