import { useMemo } from "react";
import type { UtteranceTiming } from "../hooks/useHostRoom";

interface Props {
  timings: UtteranceTiming[];
}

export function LatencyDashboard({ timings }: Props) {
  const stats = useMemo(() => {
    if (!timings.length) return null;
    const totals = timings.map((t) => t.totalMs);
    const avg = Math.round(totals.reduce((a, b) => a + b, 0) / totals.length);
    const best = Math.min(...totals);
    const gap = (avg / 300).toFixed(1);
    return { avg, best, gap, n: timings.length };
  }, [timings]);

  const maxMs = useMemo(() => {
    if (!timings.length) return 3000;
    return Math.max(...timings.map((t) => t.totalMs), 1000);
  }, [timings]);

  const TARGET_MS = 300;
  const targetPct = Math.min(99, (TARGET_MS / maxMs) * 100);

  return (
    <div className="latency-panel">
      <div className="lp-header">
        <span className="lp-title">⚡ Live Latency</span>
        {stats ? (
          <span className="lp-stats">
            Avg: <strong>{stats.avg.toLocaleString()}ms</strong>
            {" · "}
            Best: <strong>{stats.best.toLocaleString()}ms</strong>
            {" · "}
            Target: <strong className="lp-good">300ms</strong>
            {" · "}
            Gap:{" "}
            <strong className={Number(stats.gap) >= 5 ? "lp-bad" : "lp-warn"}>
              {stats.gap}x
            </strong>
            {" · "}
            n={stats.n}
          </span>
        ) : (
          <span className="lp-placeholder">
            <span className="spinner lp-spinner" /> Waiting for utterances with active guests…
          </span>
        )}
      </div>

      {timings.length > 0 && (
        <div className="lp-bars">
          {timings.map((t, i) => {
            const transW = (t.translateMs / maxMs) * 100;
            const ttsW = (t.ttsMs / maxMs) * 100;
            const overW = (t.overheadMs / maxMs) * 100;
            return (
              <div key={t.id} className="lp-row-group">
                <div className="lp-row">
                  <span className="lp-row-label">#{timings.length - i}</span>
                  <div className="lp-bar-wrap">
                    <div className="lp-bar">
                      <div className="bar-seg seg-translate" style={{ width: `${transW}%` }} />
                      <div className="bar-seg seg-tts" style={{ width: `${ttsW}%` }} />
                      <div className="bar-seg seg-overhead" style={{ width: `${overW}%` }} />
                    </div>
                    <div className="lp-target-marker" style={{ left: `${targetPct}%` }} />
                  </div>
                  <span className="lp-row-ms">{t.totalMs.toLocaleString()}ms</span>
                </div>
                <div className="lp-row-detail">
                  <span>
                    <span className="leg-dot" style={{ background: "#8b5cf6" }} />
                    {t.translateMs.toLocaleString()}ms
                  </span>
                  <span>
                    <span className="leg-dot" style={{ background: "#f59e0b" }} />
                    {t.ttsMs.toLocaleString()}ms
                  </span>
                  <span className="lp-overhead">+{t.overheadMs.toLocaleString()}ms overhead</span>
                  <span className="lp-text">"{t.text}"</span>
                </div>
              </div>
            );
          })}

          <div className="lp-legend-row">
            <span>
              <span className="leg-dot" style={{ background: "#8b5cf6" }} /> Translate
            </span>
            <span>
              <span className="leg-dot" style={{ background: "#f59e0b" }} /> TTS (Kokoro)
            </span>
            <span>
              <span className="leg-dot" style={{ background: "#555" }} /> Overhead
            </span>
            <span className="lp-scale-label">┊ = 300ms target</span>
          </div>
        </div>
      )}
    </div>
  );
}
