import { ExternalLink } from "lucide-react";
import { cn } from "../../../lib/cn";
import type { StreamInfo } from "../../../shared/api";
import { getPlatformLabel } from "../../../shared/platforms";
import { displayUrl } from "../../../shared/helpers/rtmp";
import { StreamLinks } from "../../../shared/components/StreamLinks";

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
          <StreamCard key={s.id} stream={s} isRecording={isRecording} />
        ))}
      </div>
    </section>
  );
}

function StreamCard({
  stream: s,
  isRecording,
}: {
  stream: StreamInfo;
  isRecording: boolean;
}) {
  return (
    <div className="bg-surface-container-low rounded-xl p-4 space-y-2">
      <div className="flex items-center gap-2">
        <span className="text-[10px] font-label font-bold uppercase tracking-widest text-primary bg-primary/10 px-2 py-0.5 rounded">
          {s.lang?.toUpperCase()}
        </span>
        <span className="text-on-surface font-label text-sm">
          {getPlatformLabel(s.platform ?? "custom")}
        </span>
        <StatusBadge error={s.error} isRecording={isRecording} />
      </div>

      {s.error && <p className="text-error text-xs font-label">{s.error}</p>}

      {s.rtmp_url && (
        <code className="text-on-surface-variant text-[10px] font-mono block truncate">
          {displayUrl(s.rtmp_url)}
        </code>
      )}

      {s.platform === "local-test" && s.rtmp_url && (
        <StreamLinks rtmpUrl={s.rtmp_url} />
      )}

      {s.broadcast_id && (
        <a
          href={`https://youtube.com/watch?v=${s.broadcast_id}`}
          target="_blank"
          rel="noopener noreferrer"
          className="flex items-center gap-1 text-primary text-[10px] font-label hover:underline"
        >
          <ExternalLink className="w-2.5 h-2.5" />
          YouTube
        </a>
      )}
    </div>
  );
}

function StatusBadge({
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
