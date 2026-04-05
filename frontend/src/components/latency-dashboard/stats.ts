import type { UtteranceTiming } from "../../features/host";
import { TARGET_MS, DEFAULT_MAX_MS, MIN_MAX_MS } from "./constants";

export type Stats = { avg: number; best: number; gap: string; n: number };

export function computeStats(timings: UtteranceTiming[]): Stats | null {
  if (!timings.length) return null;
  const totals = timings.map((t) => t.totalMs);
  const avg = Math.round(totals.reduce((a, b) => a + b, 0) / totals.length);
  return {
    avg,
    best: Math.min(...totals),
    gap: (avg / TARGET_MS).toFixed(1),
    n: timings.length,
  };
}

export function computeMaxMs(timings: UtteranceTiming[]): number {
  if (!timings.length) return DEFAULT_MAX_MS;
  return Math.max(...timings.map((t) => t.totalMs), MIN_MAX_MS);
}
