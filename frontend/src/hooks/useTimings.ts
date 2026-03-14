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

/**
 * Per-utterance stopwatch.
 *
 * Flow: startTimer(uid) → recordSplit("translate", ms) → finalize(uid, ttsMs)
 *
 * On finalize, computes total elapsed time and produces an UtteranceTiming
 * for the latency dashboard. Keeps the last 10 results.
 */
export function useTimings() {
  const [timings, setTimings] = useState<UtteranceTiming[]>([]);
  const pending = useRef(new Map<string, PendingTiming>());

  const startTimer = (uid: string, text: string, langs: string[]) => {
    pending.current.set(uid, { startedAt: Date.now(), text, langs, translateMs: 0 });
  };

  const recordSplit = (uid: string, translateMs: number) => {
    const entry = pending.current.get(uid);
    if (entry && !entry.translateMs) entry.translateMs = translateMs;
  };

  const finalize = (uid: string, ttsMs: number) => {
    const entry = pending.current.get(uid);
    if (!entry) return;

    pending.current.delete(uid);

    const totalMs = Date.now() - entry.startedAt;
    const timing = buildTiming(uid, entry, ttsMs, totalMs);
    setTimings((prev) => [timing, ...prev.slice(0, MAX_TIMINGS - 1)]);
  };

  const reset = () => {
    setTimings([]);
    pending.current.clear();
  };

  return { timings, startTimer, recordSplit, finalize, reset };
}

// --- internal types & helpers ---

type PendingTiming = {
  startedAt: number;
  text: string;
  langs: string[];
  translateMs: number;
};

function buildTiming(uid: string, entry: PendingTiming, ttsMs: number, totalMs: number): UtteranceTiming {
  return {
    id: uid,
    text: entry.text.slice(0, 40),
    translateMs: entry.translateMs,
    ttsMs,
    totalMs,
    overheadMs: Math.max(0, totalMs - entry.translateMs - ttsMs),
    timestamp: Date.now(),
    langs: entry.langs,
  };
}
