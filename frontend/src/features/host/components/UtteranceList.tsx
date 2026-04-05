import { useEffect, useRef } from "react";
import type { HostUtterance } from "../../../state/host/reducer";

export function UtteranceList({
  utterances,
  liveTranscript,
  sourceLangLabel,
}: {
  utterances: HostUtterance[];
  liveTranscript: string;
  sourceLangLabel: string;
}) {
  const listRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (listRef.current) {
      listRef.current.scrollTop = listRef.current.scrollHeight;
    }
  }, [utterances, liveTranscript]);

  return (
    <div ref={listRef} className="space-y-3 max-h-[400px] overflow-y-auto">
      {utterances.map((u) => (
        <div
          key={u.id}
          className="bg-surface-container-low rounded-lg px-4 py-3"
        >
          <span className="text-[10px] font-label font-bold uppercase tracking-widest text-primary bg-primary/10 px-2 py-0.5 rounded">
            {sourceLangLabel}
          </span>
          <p className="text-on-surface text-sm mt-1.5">{u.transcript}</p>
        </div>
      ))}

      {liveTranscript && (
        <div className="bg-surface-container-low/50 rounded-lg px-4 py-3 border border-primary/20">
          <span className="text-[10px] font-label font-bold uppercase tracking-widest text-primary flex items-center gap-1.5">
            <span className="w-1.5 h-1.5 bg-primary rounded-full animate-pulse" />
            Live
          </span>
          <p className="text-on-surface text-sm mt-1.5">{liveTranscript}</p>
        </div>
      )}
    </div>
  );
}
