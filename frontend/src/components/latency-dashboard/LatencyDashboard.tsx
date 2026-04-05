import { useMemo } from "react";
import { Loader2, Zap } from "lucide-react";
import { cn } from "../../lib/cn";
import type { UtteranceTiming } from "../../hooks/useHostRoom";
import { TARGET_MS, MAX_TARGET_PCT, GAP_CRITICAL_THRESHOLD } from "./constants";
import { computeStats, computeMaxMs } from "./stats";
import { TimingRow } from "./TimingRow";
import { Legend } from "./Legend";

interface Props {
  timings: UtteranceTiming[];
}

export function LatencyDashboard({ timings }: Props) {
  const stats = useMemo(() => computeStats(timings), [timings]);
  const maxMs = useMemo(() => computeMaxMs(timings), [timings]);
  const targetPct = Math.min(MAX_TARGET_PCT, (TARGET_MS / maxMs) * 100);

  return (
    <div className="bg-surface-container-low rounded-xl p-5 space-y-4">
      {/* Header */}
      <div className="flex items-center justify-between">
        <span className="font-headline font-bold text-on-surface flex items-center gap-2">
          <Zap className="w-4 h-4 text-primary" />
          Live Latency
        </span>
        {stats ? (
          <div className="flex items-center gap-3 text-xs font-label text-on-surface-variant">
            <span>
              Avg:{" "}
              <strong className="text-on-surface">
                {stats.avg.toLocaleString()}ms
              </strong>
            </span>
            <span>
              Best:{" "}
              <strong className="text-on-surface">
                {stats.best.toLocaleString()}ms
              </strong>
            </span>
            <span>
              Target:{" "}
              <strong className="text-success">{TARGET_MS}ms</strong>
            </span>
            <span>
              Gap:{" "}
              <strong
                className={cn(
                  Number(stats.gap) >= GAP_CRITICAL_THRESHOLD ? "text-error" : "text-[#f59e0b]"
                )}
              >
                {stats.gap}x
              </strong>
            </span>
            <span className="text-on-surface-variant/60">n={stats.n}</span>
          </div>
        ) : (
          <span className="flex items-center gap-2 text-xs font-label text-on-surface-variant">
            <Loader2 className="w-3 h-3 animate-spin" />
            Waiting for utterances...
          </span>
        )}
      </div>

      {/* Bars */}
      {timings.length > 0 && (
        <div className="space-y-2">
          {timings.map((t, i) => (
            <TimingRow
              key={t.id}
              timing={t}
              index={timings.length - i}
              maxMs={maxMs}
              targetPct={targetPct}
            />
          ))}
          <Legend />
        </div>
      )}
    </div>
  );
}
