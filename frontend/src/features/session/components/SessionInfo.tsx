import { cn } from "../../../lib/cn";
import type { Session } from "../../../shared/api";
import { langLabel } from "../../../shared/platforms";

type Props = {
  session: Session;
  targetLangs: string[];
};

export function SessionInfo({ session, targetLangs }: Props) {
  return (
    <section>
      <div className="flex items-center gap-4 mb-2">
        <h2 className="font-headline font-bold text-3xl tracking-tight text-on-surface">
          {session.title}
        </h2>
        <StatusBadge status={session.status} />
      </div>
      <div className="flex gap-4 text-on-surface-variant text-sm font-label">
        <span>
          Source:{" "}
          <span className="text-on-surface">{langLabel(session.source_lang)}</span>
        </span>
        <span>
          Targets:{" "}
          <span className="text-on-surface">
            {targetLangs.map((l) => langLabel(l)).join(", ")}
          </span>
        </span>
      </div>
    </section>
  );
}

function StatusBadge({ status }: { status: string }) {
  const color =
    status === "live"
      ? "text-success bg-success/10"
      : status === "ended"
        ? "text-on-surface-variant bg-surface-container-highest"
        : "text-primary bg-primary/10";

  return (
    <span
      className={cn(
        "text-[10px] font-label font-bold uppercase tracking-widest px-2 py-0.5 rounded",
        color,
      )}
    >
      {status}
    </span>
  );
}
