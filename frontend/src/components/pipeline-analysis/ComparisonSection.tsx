import type { Row } from "./pipeline-data";
import { MY_PIPELINE, BRIVVA_PIPELINE } from "./pipeline-data";
import { Table } from "./Table";

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

export function ComparisonSection() {
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
