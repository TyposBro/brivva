type Props = {
  warnings: number;
};

export function PipelineHealthBadge({ warnings }: Props) {
  if (warnings === 0) return null;

  return (
    <div className="inline-flex items-center gap-1.5 rounded-full bg-warning-container px-3 py-1 text-sm text-on-warning-container">
      <span className="text-base leading-none">{"\u26A0"}</span>
      <span>{warnings} pipeline warning{warnings !== 1 ? "s" : ""}</span>
    </div>
  );
}
