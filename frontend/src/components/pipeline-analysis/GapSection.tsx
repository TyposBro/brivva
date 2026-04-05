import { MAX_GAP_MS, TARGET_MS, TOTAL_CURRENT_MS, TOTAL_GAP_MULTIPLIER, type GapItem, GAP_ITEMS } from "./pipeline-data";

function GapBar({ label, currentMs, targetMs, note }: GapItem) {
  const currentPct = Math.min(100, (currentMs / MAX_GAP_MS) * 100);
  const targetPct = Math.min(99, (targetMs / MAX_GAP_MS) * 100);
  const multiplier = targetMs > 0 ? Math.round(currentMs / targetMs) : 0;

  return (
    <div className="pa-gap-row">
      <span className="pa-gap-label">{label}</span>
      <div className="pa-gap-bar-wrap">
        <div className="pa-gap-bar-bg">
          <div className="pa-gap-bar-fill" style={{ width: `${currentPct}%` }} />
          <div className="pa-gap-target-line" style={{ left: `${targetPct}%` }} />
        </div>
      </div>
      <span className="pa-gap-stats">
        <span className="pa-gap-current">{note ?? `${currentMs}ms`}</span>
        <span className="pa-gap-arrow"> → </span>
        <span className="pa-gap-target-val">{targetMs}ms</span>
        {multiplier > 0 && <span className="pa-gap-mult"> ({multiplier}x)</span>}
      </span>
    </div>
  );
}

export function GapSection() {
  return (
    <div className="pa-section">
      <h4 className="pa-subtitle">Gap to {TARGET_MS}ms Target</h4>
      <div className="pa-gap-bars">
        {GAP_ITEMS.map((g) => (
          <GapBar key={g.label} {...g} />
        ))}
      </div>
      <div className="pa-gap-total">
        Total: ~{TOTAL_CURRENT_MS.toLocaleString()}ms → {TARGET_MS}ms target —{" "}
        <strong className="pa-bad">{TOTAL_GAP_MULTIPLIER}x gap</strong>
      </div>
    </div>
  );
}
