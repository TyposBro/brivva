import { COLORS, TARGET_MS } from "./constants";

export function LegendDot({ color, label }: { color: string; label: string }) {
  return (
    <span className="flex items-center gap-1">
      <span
        className="w-1.5 h-1.5 rounded-full inline-block"
        style={{ background: color }}
      />
      {label}
    </span>
  );
}

export function Legend() {
  return (
    <div className="flex items-center gap-4 pt-2 pl-8 text-[9px] font-label text-on-surface-variant/60">
      <LegendDot color={COLORS.stt} label="STT" />
      <LegendDot color={COLORS.translate} label="Translate" />
      <LegendDot color={COLORS.tts} label="TTS" />
      <LegendDot color={COLORS.overhead} label="Overhead" />
      <span>| = {TARGET_MS}ms target</span>
    </div>
  );
}
