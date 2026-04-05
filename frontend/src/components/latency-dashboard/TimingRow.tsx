import type { UtteranceTiming } from "../../hooks/useHostRoom";
import { TimingBar } from "./TimingBar";
import { TimingDetails } from "./TimingDetails";

export function TimingRow({ timing: t, index, maxMs, targetPct }: {
  timing: UtteranceTiming;
  index: number;
  maxMs: number;
  targetPct: number;
}) {
  return (
    <div className="space-y-0.5">
      <div className="flex items-center gap-2">
        <span className="text-[10px] font-mono text-on-surface-variant w-6 text-right">
          #{index}
        </span>
        <TimingBar timing={t} maxMs={maxMs} targetPct={targetPct} />
        <span className="text-[10px] font-mono text-on-surface-variant w-16 text-right">
          {t.totalMs.toLocaleString()}ms
        </span>
      </div>
      <TimingDetails timing={t} />
    </div>
  );
}
