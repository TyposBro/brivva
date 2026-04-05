import type { UtteranceTiming } from "../../hooks/useHostRoom";
import { COLORS } from "./constants";

export function buildSegments(t: UtteranceTiming, maxMs: number) {
  return [
    { key: "stt", width: (t.sttMs / maxMs) * 100, color: COLORS.stt },
    { key: "translate", width: (t.translateMs / maxMs) * 100, color: COLORS.translate },
    { key: "tts", width: (t.ttsMs / maxMs) * 100, color: COLORS.tts },
    { key: "lipsync", width: (t.lipsyncMs / maxMs) * 100, color: COLORS.lipsync },
    { key: "overhead", width: (t.overheadMs / maxMs) * 100, color: COLORS.overhead },
  ];
}

export function TimingBar({ timing: t, maxMs, targetPct }: {
  timing: UtteranceTiming;
  maxMs: number;
  targetPct: number;
}) {
  const segments = buildSegments(t, maxMs);

  return (
    <div className="flex-1 relative h-5 bg-surface-container-highest rounded overflow-hidden">
      <div className="flex h-full">
        {segments.map(({ key, width, color }) =>
          width > 0 ? <div key={key} className="h-full" style={{ width: `${width}%`, background: color }} /> : null
        )}
      </div>
      <div className="absolute top-0 bottom-0 w-px bg-success/60" style={{ left: `${targetPct}%` }} />
    </div>
  );
}
