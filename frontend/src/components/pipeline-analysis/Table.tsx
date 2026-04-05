import type { Row } from "./pipeline-data";

export function Table({ headers, rows, winnerIndex, className }: {
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
