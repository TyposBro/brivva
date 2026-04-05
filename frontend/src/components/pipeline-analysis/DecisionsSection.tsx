import { useState } from "react";
import { STT_ROWS, TRANSLATION_ROWS, TTS_ROWS, LIPSYNC_ROWS } from "./pipeline-data";
import { Table } from "./Table";

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

export function DecisionsSection() {
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
