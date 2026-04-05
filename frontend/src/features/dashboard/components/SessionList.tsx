import { cn } from "../../../lib/cn";
import type { Session } from "../../../shared/api";

type Props = {
  sessions: Session[];
  onSessionClick: (id: string) => void;
};

const STATUS_STYLES: Record<string, string> = {
  live: "text-success bg-success/10",
  ended: "text-on-surface-variant bg-surface-container-highest",
};

const DEFAULT_STATUS_STYLE = "text-primary bg-primary/10";

export function SessionList({ sessions, onSessionClick }: Props) {
  if (sessions.length === 0) return null;

  return (
    <div className="pt-6">
      <span className="text-xs font-label font-bold uppercase tracking-widest text-on-surface-variant block mb-3">
        Recent Sessions
      </span>
      <div className="space-y-1.5">
        {sessions.map((s) => (
          <div
            key={s.id}
            className="flex items-center gap-3 bg-surface-container-high hover:bg-surface-bright px-4 py-3 rounded-lg cursor-pointer transition-colors"
            onClick={() => onSessionClick(s.id)}
          >
            <span className="text-on-surface font-label text-sm flex-1 truncate">
              {s.title}
            </span>
            <span
              className={cn(
                "text-[10px] font-label font-bold uppercase tracking-widest px-2 py-0.5 rounded shrink-0",
                STATUS_STYLES[s.status] ?? DEFAULT_STATUS_STYLE,
              )}
            >
              {s.status}
            </span>
          </div>
        ))}
      </div>
    </div>
  );
}
