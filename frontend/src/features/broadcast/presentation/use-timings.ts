import { useState, useRef, useCallback } from "react";

const MAX_TIMINGS = 10;

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
};

/**
 * Per-utterance stopwatch.
 *
 * Flow: markInterim() → startTimer(uid) [on final] → recordTranslate → recordTts (finalizes)
 *
 * STT time = time from first interim to final event.
 * Total measured from first interim (speech detected) to tts_end (audio delivered).
 */
export function useTimings() {
  const [timings, setTimings] = useState<UtteranceTiming[]>([]);
  const pending = useRef(new Map<string, PendingTiming>());
  const interimStartedAt = useRef<number | null>(null);

  /** Call on each interim — records when speech was first detected */
  const markInterim = useCallback(() => {
    if (interimStartedAt.current === null) {
      interimStartedAt.current = Date.now();
    }
  }, []);

  const startTimer = useCallback((uid: string, text: string, langs: string[]) => {
    const sttMs = interimStartedAt.current ? Date.now() - interimStartedAt.current : 0;
    interimStartedAt.current = null; // reset for next utterance
    pending.current.set(uid, { startedAt: Date.now() - sttMs, text, langs, sttMs, translateMs: 0, ttsMs: 0 });
  }, []);

  const recordStt = useCallback((uid: string, sttMs: number) => {
    const entry = pending.current.get(uid);
    if (entry) entry.sttMs = sttMs;
  }, []);

  const recordTranslate = useCallback((uid: string, translateMs: number) => {
    const entry = pending.current.get(uid);
    if (entry && !entry.translateMs) entry.translateMs = translateMs;
  }, []);

  /** Records TTS time and immediately finalizes (no lip-sync step) */
  const recordTts = useCallback((uid: string, ttsMs: number) => {
    const entry = pending.current.get(uid);
    if (!entry) return;
    entry.ttsMs = ttsMs;
    pending.current.delete(uid);

    const totalMs = Date.now() - entry.startedAt;
    const timing: UtteranceTiming = {
      id: uid,
      text: entry.text.slice(0, 40),
      sttMs: entry.sttMs,
      translateMs: entry.translateMs,
      ttsMs,
      lipsyncMs: 0,
      totalMs,
      overheadMs: Math.max(0, totalMs - entry.sttMs - entry.translateMs - ttsMs),
      timestamp: Date.now(),
      langs: entry.langs,
    };
    setTimings((prev) => [timing, ...prev.slice(0, MAX_TIMINGS - 1)]);
  }, []);

  const finalize = useCallback((_uid: string, _lipsyncMs: number) => {
    // No-op: kept for interface compatibility, tts_end now finalizes directly
  }, []);

  const reset = useCallback(() => {
    setTimings([]);
    pending.current.clear();
    interimStartedAt.current = null;
  }, []);

  return { timings, startTimer, markInterim, recordStt, recordTranslate, recordTts, finalize, reset };
}
