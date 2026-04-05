import { cn } from "../../../../lib/cn";

export function StatusBadge({
  error,
  isRecording,
}: {
  error?: string;
  isRecording: boolean;
}) {
  const label = error ? "ERR" : isRecording ? "LIVE" : "READY";
  const color = error
    ? "text-error bg-error-container/30"
    : isRecording
      ? "text-success bg-success/10"
      : "text-on-surface-variant bg-surface-container-highest";

  return (
    <span
      className={cn(
        "ml-auto text-[10px] font-label font-bold uppercase tracking-widest px-2 py-0.5 rounded",
        color,
      )}
    >
      {label}
    </span>
  );
}
