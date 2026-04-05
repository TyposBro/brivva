import type { StreamInfo } from "../../../shared/api";
import { HostStreamCard } from "./stream-cards/HostStreamCard";

export function StreamCards({
  streams,
  isRecording,
}: {
  streams: StreamInfo[];
  isRecording: boolean;
}) {
  if (streams.length === 0) return null;

  return (
    <section>
      <div className="flex items-center gap-3 mb-3">
        <h3 className="font-headline font-bold text-lg text-on-surface">
          Live Streams
        </h3>
        <span className="text-[10px] font-label font-bold uppercase tracking-widest text-on-surface-variant bg-surface-container-highest px-2 py-0.5 rounded">
          {streams.length} platform{streams.length !== 1 ? "s" : ""}
        </span>
      </div>

      <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-3">
        {streams.map((s) => (
          <HostStreamCard key={s.id} stream={s} isRecording={isRecording} />
        ))}
      </div>
    </section>
  );
}
