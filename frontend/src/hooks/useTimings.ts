import { useState, useRef, useCallback } from "react";

const MAX_TIMINGS = 10;
const AUTO_FINALIZE_MS = 3000;

export interface UtteranceTiming {
  id: string;
  text: string;
  sttMs: number;
  translateMs: number;
  ttsMs: number;
  lipsyncMs: number;
  totalMs: number;
  overheadMs: number;
  timestamp: number;
  langs: string[];
}

type PendingTiming = {
  startedAt: number;
  text: string;
  langs: string[];
  sttMs: number;
  translateMs: number;
  ttsMs: number;
  timer: ReturnType<typeof setTimeout> | null;
};

/**
 * Per-utterance stopwatch.
 *
 * Flow: startTimer(uid) → recordStt(uid, ms) → recordTranslate(uid, ms) → recordTts(uid, ms)
 *       → finalize(uid, lipsyncMs) or auto-finalize after 3s timeout
 */
export function useTimings() {
  const [timings, setTimings] = useState<UtteranceTiming[]>([]);
  const pending = useRef(new Map<string, PendingTiming>());

  const doFinalize = useCallback((uid: string, lipsyncMs: number) => {
    const entry = pending.current.get(uid);
    if (!entry) return;

    if (entry.timer) clearTimeout(entry.timer);
    pending.current.delete(uid);

    const totalMs = Date.now() - entry.startedAt;
    const timing: UtteranceTiming = {
      id: uid,
      text: entry.text.slice(0, 40),
      sttMs: entry.sttMs,
      translateMs: entry.translateMs,
      ttsMs: entry.ttsMs,
      lipsyncMs,
      totalMs,
      overheadMs: Math.max(0, totalMs - entry.sttMs - entry.translateMs - entry.ttsMs - lipsyncMs),
      timestamp: Date.now(),
      langs: entry.langs,
    };
    setTimings((prev) => [timing, ...prev.slice(0, MAX_TIMINGS - 1)]);
  }, []);

  const startTimer = useCallback((uid: string, text: string, langs: string[]) => {
    pending.current.set(uid, { startedAt: Date.now(), text, langs, sttMs: 0, translateMs: 0, ttsMs: 0, timer: null });
  }, []);

  const recordStt = useCallback((uid: string, sttMs: number) => {
    const entry = pending.current.get(uid);
    if (entry && !entry.sttMs) entry.sttMs = sttMs;
  }, []);

  const recordTranslate = useCallback((uid: string, translateMs: number) => {
    const entry = pending.current.get(uid);
    if (entry && !entry.translateMs) entry.translateMs = translateMs;
  }, []);

  const recordTts = useCallback((uid: string, ttsMs: number) => {
    const entry = pending.current.get(uid);
    if (!entry) return;
    entry.ttsMs = ttsMs;
    // Auto-finalize after timeout if no video_end arrives (audio-only fallback)
    entry.timer = setTimeout(() => doFinalize(uid, 0), AUTO_FINALIZE_MS);
  }, [doFinalize]);

  const finalize = useCallback((uid: string, lipsyncMs: number) => {
    doFinalize(uid, lipsyncMs);
  }, [doFinalize]);

  const reset = useCallback(() => {
    // Clear any pending timers
    for (const entry of pending.current.values()) {
      if (entry.timer) clearTimeout(entry.timer);
    }
    setTimings([]);
    pending.current.clear();
  }, []);

  return { timings, startTimer, recordStt, recordTranslate, recordTts, finalize, reset };
}
