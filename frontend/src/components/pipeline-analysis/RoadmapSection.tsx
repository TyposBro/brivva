import { TARGET_MS, ROADMAP } from "./pipeline-data";
import { Table } from "./Table";

export function RoadmapSection() {
  return (
    <div className="pa-section">
      <h4 className="pa-subtitle">Optimization Roadmap</h4>
      <Table headers={["Optimization", "Impact", "Status"]} rows={ROADMAP} className="pa-roadmap" />
      <p className="pa-footer">True {TARGET_MS}ms needs co-located models or end-to-end S2S. That's Brivva's R&D challenge.</p>
    </div>
  );
}
