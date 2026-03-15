import { useState } from "react";

// --- static data ---

const MAX_GAP_MS = 1500;
const TARGET_MS = 300;
const TOTAL_CURRENT_MS = 862;
const TOTAL_GAP_MULTIPLIER = 2.9;

type Row = string[];

const MY_PIPELINE: Row[] = [
  ["STT", "CF Nova-3 (via stt-wrapper)", "streaming ✓"],
  ["Translation", "NLLB (self-hosted GPU)", "82–164ms"],
  ["TTS", "ElevenLabs flash_v2_5", "548–1,440ms"],
  ["Server", "Rust axum + DashMap", "fan-out/lang"],
  ["Lip-sync", "—", "not built"],
];

const BRIVVA_PIPELINE: Row[] = [
  ["STT", "Whisper", "batch only"],
  ["Translation", "Context NMT", "unknown"],
  ["TTS", "Emotive TTS (3B)", "unknown"],
  ["Lip-sync", "Wav2Lip", "lips only"],
  ["Target", "—", "<300ms e2e"],
];

const STT_ROWS: Row[] = [
  ["✓ CF Nova-3 (stt-wrapper)", "6.5%", "Yes — streaming", "CF AI Gateway"],
  ["WhisperLiveKit (base.en)", "—", "Yes — live interims", "Free (self-hosted)"],
  ["Whisper v3 Turbo", "4.8%", "No — batch only", "$0.67/1000min"],
];

const TRANSLATION_ROWS: Row[] = [
  ["✓ NLLB (self-hosted GPU)", "Seq2Seq", "82–164ms"],
  ["M2M100-1.2B (CF Workers AI)", "Seq2Seq", "394–870ms"],
  ["Llama 3.2 1B", "LLM prompted", "~1,500ms"],
];

const TTS_ROWS: Row[] = [
  ["✓ ElevenLabs flash_v2_5", "API", "548–1,440ms", "Yes — 32 langs"],
  ["Kokoro 82M (self-hosted)", "82M", "111–358ms GPU", "Limited"],
  ["Emotive TTS (Brivva)", "3B", "unknown", "Yes — emotions"],
];

const GAP_ITEMS = [
  { label: "Translate", currentMs: 123, targetMs: 50 },
  { label: "TTS", currentMs: 728, targetMs: 200 },
  { label: "Overhead", currentMs: 11, targetMs: 10 },
];

const ROADMAP: Row[] = [
  ["Self-hosted pipeline (Rust)", "−1700ms avg", "Done ✓"],
  ["STT wrapper (clean events)", "−complexity", "Done ✓"],
  ["Parallel translations", "—", "Done ✓"],
  ["GPU inference (NLLB + Kokoro)", "−5x latency", "Done ✓"],
  ["Streaming TTS playback", "−100ms perceived", "Medium"],
  ["Co-locate all models on one GPU", "−20ms network", "Brivva infra"],
  ["End-to-end speech-to-speech", "paradigm shift", "Brivva's R&D goal"],
];

// --- main component ---

export function PipelineAnalysis() {
  const [open, setOpen] = useState(false);

  return (
    <div className="pipeline-analysis">
      <button className="pa-main-toggle" onClick={() => setOpen((v) => !v)}>
        <span className="pa-main-chevron">{open ? "▾" : "▸"}</span>
        Pipeline Analysis
        {!open && <span className="pa-main-hint"> — my choices vs Brivva's pipeline, gap to {TARGET_MS}ms</span>}
      </button>

      {open && (
        <div className="pa-body">
          <ComparisonSection />
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
      <h4 className="pa-subtitle">My Prototype vs Brivva's Pipeline</h4>
      <div className="pa-compare">
        <PipelineColumn title="My Prototype" className="pa-col-mine" headers={["Phase", "Tech", "Measured"]} rows={MY_PIPELINE} />
        <PipelineColumn title="Brivva's Pipeline" className="pa-col-brivva" headers={["Phase", "Tech", "Status"]} rows={BRIVVA_PIPELINE} />
      </div>
    </div>
  );
}

function DecisionsSection() {
  return (
    <div className="pa-section">
      <h4 className="pa-subtitle">Component Decisions</h4>
      <Collapsible title="STT — Why CF Nova-3 (stt-wrapper)">
        <Table headers={["Model", "WER", "Streaming", "Price"]} rows={STT_ROWS} winnerIndex={0} />
        <p className="pa-verdict">Streaming is non-negotiable for live translation. Self-hosted eliminates API costs and latency to cloud.</p>
      </Collapsible>
      <Collapsible title="Translation — Why NLLB (self-hosted)">
        <Table headers={["Model", "Type", "Latency"]} rows={TRANSLATION_ROWS} winnerIndex={0} />
        <p className="pa-verdict">Self-hosted NLLB removes CF dependency. Same seq2seq architecture, no network roundtrip to edge.</p>
      </Collapsible>
      <Collapsible title="TTS — Why ElevenLabs (and its tradeoffs)">
        <Table headers={["Model", "Params", "Latency", "Expressive"]} rows={TTS_ROWS} winnerIndex={0} />
        <p className="pa-verdict">ElevenLabs gives 32-language support with natural voices. No GPU needed. Tradeoff: API dependency + per-character cost.</p>
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
      <Table headers={["Optimization", "Savings", "Status"]} rows={ROADMAP} className="pa-roadmap" />
      <p className="pa-footer">True {TARGET_MS}ms needs co-located models or end-to-end S2S. That's the R&D challenge.</p>
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

function GapBar({ label, currentMs, targetMs }: { label: string; currentMs: number; targetMs: number }) {
  const currentPct = Math.min(100, (currentMs / MAX_GAP_MS) * 100);
  const targetPct = Math.min(99, (targetMs / MAX_GAP_MS) * 100);
  const multiplier = Math.round(currentMs / targetMs);

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
        <span className="pa-gap-current">{currentMs}ms</span>
        <span className="pa-gap-arrow"> → </span>
        <span className="pa-gap-target-val">{targetMs}ms</span>
        <span className="pa-gap-mult"> ({multiplier}x)</span>
      </span>
    </div>
  );
}
