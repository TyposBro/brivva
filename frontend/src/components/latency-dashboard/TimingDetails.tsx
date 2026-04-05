import type { UtteranceTiming } from "../../hooks/useHostRoom";
import { COLORS } from "./constants";
import { LegendDot } from "./Legend";

export function TimingDetails({ timing: t }: { timing: UtteranceTiming }) {
  return (
    <div className="flex items-center gap-3 pl-8 text-[9px] font-label text-on-surface-variant">
      {t.sttMs > 0 && <LegendDot color={COLORS.stt} label={`${t.sttMs.toLocaleString()}ms`} />}
      <LegendDot color={COLORS.translate} label={`${t.translateMs.toLocaleString()}ms`} />
      <LegendDot color={COLORS.tts} label={`${t.ttsMs.toLocaleString()}ms`} />
      {t.lipsyncMs > 0 && <LegendDot color={COLORS.lipsync} label={`${t.lipsyncMs.toLocaleString()}ms`} />}
      <span className="text-on-surface-variant/50">+{t.overheadMs.toLocaleString()}ms</span>
      <span className="text-on-surface-variant/40 truncate max-w-[200px]">"{t.text}"</span>
    </div>
  );
}
