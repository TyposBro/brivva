import { useState } from "react";
import { TARGET_MS } from "./pipeline-data";
import { ComparisonSection } from "./ComparisonSection";
import { KnownIssuesSection } from "./KnownIssuesSection";
import { DecisionsSection } from "./DecisionsSection";
import { GapSection } from "./GapSection";
import { RoadmapSection } from "./RoadmapSection";

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
