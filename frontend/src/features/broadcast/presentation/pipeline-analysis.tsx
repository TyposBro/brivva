import { useState } from "react";

// --- static data ---

const MAX_GAP_MS = 1500;
const TARGET_MS = 300;
const TOTAL_CURRENT_MS = 862;
const TOTAL_GAP_MULTIPLIER = 2.9;

type Row = string[];

const MY_PIPELINE: Row[] = [
  ["STT", "CF Nova-3 (streaming via stt-wrapper)", "streaming"],
  ["Translation", "NLLB-200-distilled-600M (self-hosted)", "170ms avg"],
  ["TTS", "ElevenLabs eleven_flash_v2_5", "539ms avg"],
  ["Lip-sync", "Wav2Lip + GFPGAN (self-hosted GPU)", "built, testing"],
  ["Voice Clone", "ElevenLabs IVC (5s PCM)", "not working"],
  ["Emotion TTS", "Prosody extraction + style mapping", "not working"],
  ["Server", "Rust axum + DashMap + fan-out/lang", "orchestration"],
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
  ["✓ NLLB-200-distilled-600M (GPU)", "Seq2Seq", "82–164ms"],
  ["M2M100-1.2B (CF Workers AI)", "Seq2Seq", "394–870ms"],
  ["Llama 3.2 1B", "LLM prompted", "~1,500ms"],
];

const TTS_ROWS: Row[] = [
  ["✓ ElevenLabs eleven_flash_v2_5", "API", "548–1,440ms", "Yes — 32 langs"],
  ["Kokoro 82M (self-hosted)", "82M", "111–358ms GPU", "Limited"],
  ["Emotive TTS (Brivva)", "3B", "unknown", "Yes — emotions"],
];

const LIPSYNC_ROWS: Row[] = [
  ["✓ Wav2Lip + GFPGAN (default)", "Batched 16/fr", "TBD", "Fast, lower quality"],
  ["MuseTalk v1.5", "Per-frame UNet", "~188ms/frame", "Higher quality, 5x slow"],
];

const GAP_ITEMS = [
  { label: "Translate", currentMs: 170, targetMs: 50 },
  { label: "TTS", currentMs: 539, targetMs: 200 },
  { label: "Lip-sync", currentMs: 0, targetMs: 50, note: "TBD" },
  { label: "Overhead", currentMs: 11, targetMs: 10 },
];

const ROADMAP: Row[] = [
  ["Self-hosted pipeline (Rust)", "−1700ms avg", "Done ✓"],
  ["STT wrapper (clean events)", "−complexity", "Done ✓"],
  ["Parallel translations per lang", "—", "Done ✓"],
  ["GPU inference (NLLB on A10G)", "−5x latency", "Done ✓"],
  ["Wav2Lip + GFPGAN lip-sync", "visual quality", "Built, testing"],
  ["Voice cloning (ElevenLabs IVC)", "host voice preservation", "Not working"],
  ["Emotion-conditioned TTS", "expressive translation", "Not working"],
  ["Streaming TTS playback", "−100ms perceived", "Medium"],
  ["Co-locate all models on one GPU", "−20ms network", "Brivva infra"],
  ["End-to-end speech-to-speech", "paradigm shift", "Brivva's R&D goal"],
];

const KNOWN_ISSUES: Row[] = [
  ["Voice cloning", "ElevenLabs /v1/voices/add returns error — likely API key tier or PCM format issue. Falls back to default per-language voices."],
  ["Emotion/prosody TTS", "stt-wrapper prosody extraction code exists but style_params not reaching ElevenLabs — always uses default voice_settings."],
  ["Lip-sync frozen body", "Wav2Lip/MuseTalk only modify mouth on single frame. Host body/gestures freeze during 2-5s processing. Industry-wide limitation."],
];

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

function GapBar({ label, currentMs, targetMs, note }: { label: string; currentMs: number; targetMs: number; note?: string }) {
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
