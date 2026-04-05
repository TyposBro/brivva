import { KNOWN_ISSUES } from "./pipeline-data";

export function KnownIssuesSection() {
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
