import { cn } from "../../../../lib/cn";
import type { StreamInfo } from "../../../../shared/api";
import { getPlatformLabel } from "../../../../shared/platforms";

export function StreamCardHeader({ stream: s }: { stream: StreamInfo }) {
  const statusLabel = s.error ? "Error" : s.status ?? "pending";
  const statusColor = s.error
    ? "text-error bg-error-container/30"
    : s.status === "ready"
      ? "text-success bg-success/10"
      : "text-on-surface-variant bg-surface-container-highest";

  return (
    <div className="flex items-center gap-3">
      <span className="text-[10px] font-label font-bold uppercase tracking-widest text-primary bg-primary/10 px-2 py-0.5 rounded">
        {s.lang?.toUpperCase()}
      </span>
      {s.platform && (
        <span className="text-on-surface font-label text-sm">
          {getPlatformLabel(s.platform)}
        </span>
      )}
      <span
        className={cn(
          "ml-auto text-[10px] font-label font-bold uppercase tracking-widest px-2 py-0.5 rounded",
          statusColor,
        )}
      >
        {statusLabel}
      </span>
    </div>
  );
}
