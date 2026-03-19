import { useMemo } from "react";
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
    <div className="latency-panel">
      <Header stats={stats} />
      {timings.length > 0 && (
        <div className="lp-bars">
          {timings.map((t, i) => (
            <TimingRow key={t.id} timing={t} index={timings.length - i} maxMs={maxMs} targetPct={targetPct} />
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

function Header({ stats }: { stats: Stats | null }) {
  return (
    <div className="lp-header">
      <span className="lp-title">⚡ Live Latency</span>
      {stats ? <StatsBar stats={stats} /> : <WaitingMessage />}
    </div>
  );
}

function StatsBar({ stats }: { stats: Stats }) {
  const gapClass = Number(stats.gap) >= 5 ? "lp-bad" : "lp-warn";
  return (
    <span className="lp-stats">
      Avg: <strong>{stats.avg.toLocaleString()}ms</strong>
      {" · "}
      Best: <strong>{stats.best.toLocaleString()}ms</strong>
      {" · "}
      Target: <strong className="lp-good">{TARGET_MS}ms</strong>
      {" · "}
      Gap: <strong className={gapClass}>{stats.gap}x</strong>
      {" · "}
      n={stats.n}
    </span>
  );
}

function WaitingMessage() {
  return (
    <span className="lp-placeholder">
      <span className="spinner lp-spinner" /> Waiting for utterances with active guests…
    </span>
  );
}

function TimingRow({ timing: t, index, maxMs, targetPct }: {
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
    <div className="lp-row-group">
      <div className="lp-row">
        <span className="lp-row-label">#{index}</span>
        <div className="lp-bar-wrap">
          <div className="lp-bar">
            {sttW > 0 && <Segment className="seg-stt" width={sttW} />}
            <Segment className="seg-translate" width={transW} />
            <Segment className="seg-tts" width={ttsW} />
            {lipsyncW > 0 && <Segment className="seg-lipsync" width={lipsyncW} />}
            <Segment className="seg-overhead" width={overW} />
          </div>
          <div className="lp-target-marker" style={{ left: `${targetPct}%` }} />
        </div>
        <span className="lp-row-ms">{t.totalMs.toLocaleString()}ms</span>
      </div>
      <div className="lp-row-detail">
        {t.sttMs > 0 && <LegendDot color={COLORS.stt} label={`${t.sttMs.toLocaleString()}ms`} />}
        <LegendDot color={COLORS.translate} label={`${t.translateMs.toLocaleString()}ms`} />
        <LegendDot color={COLORS.tts} label={`${t.ttsMs.toLocaleString()}ms`} />
        {t.lipsyncMs > 0 && <LegendDot color={COLORS.lipsync} label={`${t.lipsyncMs.toLocaleString()}ms`} />}
        <span className="lp-overhead">+{t.overheadMs.toLocaleString()}ms overhead</span>
        <span className="lp-text">"{t.text}"</span>
      </div>
    </div>
  );
}

function Segment({ className, width }: { className: string; width: number }) {
  return <div className={`bar-seg ${className}`} style={{ width: `${width}%` }} />;
}

function LegendDot({ color, label }: { color: string; label: string }) {
  return (
    <span>
      <span className="leg-dot" style={{ background: color }} />
      {label}
    </span>
  );
}

function Legend() {
  return (
    <div className="lp-legend-row">
      <LegendDot color={COLORS.stt} label=" Nova-3 STT" />
      <LegendDot color={COLORS.translate} label=" Translate" />
      <LegendDot color={COLORS.tts} label=" TTS (ElevenLabs)" />
      <LegendDot color={COLORS.lipsync} label=" Lip-sync" />
      <LegendDot color={COLORS.overhead} label=" Overhead" />
      <span className="lp-scale-label">┊ = {TARGET_MS}ms target</span>
    </div>
  );
}
