type Props = {
  warnings: string[];
};

export function PipelineHealthBadge({ warnings }: Props) {
  if (warnings.length === 0) return null;

  return (
    <div className="space-y-1">
      <div className="inline-flex items-center gap-1.5 rounded-full bg-warning-container px-3 py-1 text-sm text-on-warning-container">
        <span className="text-base leading-none">{"\u26A0"}</span>
        <span>{warnings.length} pipeline warning{warnings.length !== 1 ? "s" : ""}</span>
      </div>
      <ul className="space-y-0.5 pl-1 text-xs text-on-warning-container/80">
        {warnings.map((w, i) => (
          <li key={i} className="font-mono">{w}</li>
        ))}
      </ul>
    </div>
  );
}
