import { useState } from "react";
import {
  MAX_GAP_MS, TARGET_MS, TOTAL_CURRENT_MS, TOTAL_GAP_MULTIPLIER,
  type Row, type GapItem,
  MY_PIPELINE, BRIVVA_PIPELINE, STT_ROWS, TRANSLATION_ROWS,
  TTS_ROWS, LIPSYNC_ROWS, GAP_ITEMS, ROADMAP, KNOWN_ISSUES,
} from "./pipeline-data";

// --- main component ---

export function PipelineAnalysis() {
  const [open, setOpen] = useState(false);

  return (
    <div className="pipeline-analysis">
      <button className="pa-main-toggle" onClick={() => setOpen((v) => !v)}>
        <span className="pa-main-chevron">{open ? "▾" : "▸"}</span>
        Pipeline Analysis
        {!open && <span className="pa-main-hint"> — v5 pipeline, gap to {TARGET_MS}ms, known issues</span>}
      </button>

      {open && (
        <div className="pa-body">
          <ComparisonSection />
          <KnownIssuesSection />
          <DecisionsSection />
          <GapSection />
          <RoadmapSection />
        </div>
      )}
    </div>
  );
}

// --- sections ---

function ComparisonSection() {
  return (
    <div className="pa-section">
      <h4 className="pa-subtitle">My Prototype (v5) vs Brivva's Pipeline</h4>
      <div className="pa-compare">
        <PipelineColumn title="My Prototype" className="pa-col-mine" headers={["Phase", "Tech", "Status"]} rows={MY_PIPELINE} />
        <PipelineColumn title="Brivva's Pipeline" className="pa-col-brivva" headers={["Phase", "Tech", "Status"]} rows={BRIVVA_PIPELINE} />
      </div>
    </div>
  );
}

function KnownIssuesSection() {
  return (
    <div className="pa-section">
      <h4 className="pa-subtitle">Known Issues (v5)</h4>
      <table className="pa-table">
        <thead>
          <tr><th>Feature</th><th>Issue</th></tr>
        </thead>
        <tbody>
          {KNOWN_ISSUES.map((row, i) => (
            <tr key={i} className="pa-issue-row">
              {row.map((cell, j) => <td key={j}>{cell}</td>)}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function DecisionsSection() {
  return (
    <div className="pa-section">
      <h4 className="pa-subtitle">Component Decisions</h4>
      <Collapsible title="STT — CF Nova-3 (streaming via stt-wrapper)">
        <Table headers={["Model", "WER", "Streaming", "Price"]} rows={STT_ROWS} winnerIndex={0} />
        <p className="pa-verdict">Streaming is non-negotiable for live translation. Nova-3 provides interims while speaking with no GPU needed.</p>
      </Collapsible>
      <Collapsible title="Translation — NLLB-200-distilled-600M (self-hosted)">
        <Table headers={["Model", "Type", "Latency"]} rows={TRANSLATION_ROWS} winnerIndex={0} />
        <p className="pa-verdict">Self-hosted NLLB on A10G GPU. No network roundtrip, 200 languages, half the size of M2M100-1.2B.</p>
      </Collapsible>
      <Collapsible title="TTS — ElevenLabs eleven_flash_v2_5">
        <Table headers={["Model", "Params", "Latency", "Expressive"]} rows={TTS_ROWS} winnerIndex={0} />
        <p className="pa-verdict">32-language support with natural voices. No GPU needed. Tradeoff: API dependency + per-character cost. Voice cloning supported but not yet working.</p>
      </Collapsible>
      <Collapsible title="Lip-sync — Wav2Lip + GFPGAN (dual backend)">
        <Table headers={["Backend", "Inference", "Latency", "Notes"]} rows={LIPSYNC_ROWS} winnerIndex={0} />
        <p className="pa-verdict">Wav2Lip (batched, GFPGAN enhanced) is default. MuseTalk switchable via env var. Both share same /lipsync API. Single-frame limitation is an industry problem.</p>
      </Collapsible>
    </div>
  );
}

function GapSection() {
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

function RoadmapSection() {
  return (
    <div className="pa-section">
      <h4 className="pa-subtitle">Optimization Roadmap</h4>
      <Table headers={["Optimization", "Impact", "Status"]} rows={ROADMAP} className="pa-roadmap" />
      <p className="pa-footer">True {TARGET_MS}ms needs co-located models or end-to-end S2S. That's Brivva's R&D challenge.</p>
    </div>
  );
}

// --- reusable pieces ---

function Collapsible({ title, children }: { title: string; children: React.ReactNode }) {
  const [open, setOpen] = useState(false);
  return (
    <div className="pa-collapsible">
      <button className="pa-toggle" onClick={() => setOpen((v) => !v)}>
        <span className="pa-chevron">{open ? "▾" : "▸"}</span>
        {title}
      </button>
      {open && <div className="pa-content">{children}</div>}
    </div>
  );
}

function Table({ headers, rows, winnerIndex, className }: {
  headers: string[];
  rows: Row[];
  winnerIndex?: number;
  className?: string;
}) {
  return (
    <table className={`pa-table ${className ?? ""}`}>
      <thead>
        <tr>{headers.map((h) => <th key={h}>{h}</th>)}</tr>
      </thead>
      <tbody>
        {rows.map((row, i) => (
          <tr key={i} className={i === winnerIndex ? "pa-winner" : ""}>
            {row.map((cell, j) => <td key={j}>{cell}</td>)}
          </tr>
        ))}
      </tbody>
    </table>
  );
}

function PipelineColumn({ title, className, headers, rows }: {
  title: string;
  className: string;
  headers: string[];
  rows: Row[];
}) {
  return (
    <div className="pa-col">
      <div className={`pa-col-header ${className}`}>{title}</div>
      <Table headers={headers} rows={rows} />
    </div>
  );
}

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
