import { useMemo } from "react";
import { Loader2, Zap } from "lucide-react";
import { cn } from "../lib/cn";
import type { UtteranceTiming } from "../hooks/useHostRoom";

const TARGET_MS = 300;
const DEFAULT_MAX_MS = 3000;
const MIN_MAX_MS = 1000;
const MAX_TARGET_PCT = 99;

const COLORS = {
  stt: "#38bdf8",
  translate: "#8b5cf6",
  tts: "#f59e0b",
  lipsync: "#f43f5e",
  overhead: "#555",
};

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
                  Number(stats.gap) >= 5 ? "text-error" : "text-[#f59e0b]"
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

// --- stats ---

type Stats = { avg: number; best: number; gap: string; n: number };

function computeStats(timings: UtteranceTiming[]): Stats | null {
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

function computeMaxMs(timings: UtteranceTiming[]): number {
  if (!timings.length) return DEFAULT_MAX_MS;
  return Math.max(...timings.map((t) => t.totalMs), MIN_MAX_MS);
}

// --- sub-components ---

function TimingRow({
  timing: t,
  index,
  maxMs,
  targetPct,
}: {
  timing: UtteranceTiming;
  index: number;
  maxMs: number;
  targetPct: number;
}) {
  const sttW = (t.sttMs / maxMs) * 100;
  const transW = (t.translateMs / maxMs) * 100;
  const ttsW = (t.ttsMs / maxMs) * 100;
  const lipsyncW = (t.lipsyncMs / maxMs) * 100;
  const overW = (t.overheadMs / maxMs) * 100;

  return (
    <div className="space-y-0.5">
      <div className="flex items-center gap-2">
        <span className="text-[10px] font-mono text-on-surface-variant w-6 text-right">
          #{index}
        </span>
        <div className="flex-1 relative h-5 bg-surface-container-highest rounded overflow-hidden">
          <div className="flex h-full">
            {sttW > 0 && (
              <div
                className="h-full"
                style={{ width: `${sttW}%`, background: COLORS.stt }}
              />
            )}
            <div
              className="h-full"
              style={{ width: `${transW}%`, background: COLORS.translate }}
            />
            <div
              className="h-full"
              style={{ width: `${ttsW}%`, background: COLORS.tts }}
            />
            {lipsyncW > 0 && (
              <div
                className="h-full"
                style={{ width: `${lipsyncW}%`, background: COLORS.lipsync }}
              />
            )}
            <div
              className="h-full"
              style={{ width: `${overW}%`, background: COLORS.overhead }}
            />
          </div>
          {/* Target marker */}
          <div
            className="absolute top-0 bottom-0 w-px bg-success/60"
            style={{ left: `${targetPct}%` }}
          />
        </div>
        <span className="text-[10px] font-mono text-on-surface-variant w-16 text-right">
          {t.totalMs.toLocaleString()}ms
        </span>
      </div>
      <div className="flex items-center gap-3 pl-8 text-[9px] font-label text-on-surface-variant">
        {t.sttMs > 0 && (
          <LegendDot
            color={COLORS.stt}
            label={`${t.sttMs.toLocaleString()}ms`}
          />
        )}
        <LegendDot
          color={COLORS.translate}
          label={`${t.translateMs.toLocaleString()}ms`}
        />
        <LegendDot
          color={COLORS.tts}
          label={`${t.ttsMs.toLocaleString()}ms`}
        />
        {t.lipsyncMs > 0 && (
          <LegendDot
            color={COLORS.lipsync}
            label={`${t.lipsyncMs.toLocaleString()}ms`}
          />
        )}
        <span className="text-on-surface-variant/50">
          +{t.overheadMs.toLocaleString()}ms
        </span>
        <span className="text-on-surface-variant/40 truncate max-w-[200px]">
          "{t.text}"
        </span>
      </div>
    </div>
  );
}

function LegendDot({ color, label }: { color: string; label: string }) {
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

function Legend() {
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
