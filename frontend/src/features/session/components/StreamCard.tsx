import { ExternalLink, Copy } from "lucide-react";
import { cn } from "../../../lib/cn";
import type { StreamInfo } from "../../../shared/api";
import { getPlatformLabel } from "../../../shared/platforms";
import { displayUrl, streamPath } from "../../../shared/helpers/rtmp";

export function StreamCard({ stream: s }: { stream: StreamInfo }) {
  return (
    <div className="bg-surface-container-low rounded-xl p-5 space-y-3">
      <CardHeader stream={s} />

      {s.error && <p className="text-error text-xs font-label">{s.error}</p>}

      <div className="space-y-1.5">
        {s.broadcast_id && <YouTubeLink broadcastId={s.broadcast_id} />}
        {s.rtmp_url && <RtmpUrl url={s.rtmp_url} />}
        {s.platform === "local-test" && s.rtmp_url && (
          <LocalTestLinks rtmpUrl={s.rtmp_url} />
        )}
      </div>
    </div>
  );
}

function CardHeader({ stream: s }: { stream: StreamInfo }) {
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

function YouTubeLink({ broadcastId }: { broadcastId: string }) {
  return (
    <a
      href={`https://youtube.com/watch?v=${broadcastId}`}
      target="_blank"
      rel="noopener noreferrer"
      className="flex items-center gap-1.5 text-primary text-xs font-label hover:underline"
    >
      <ExternalLink className="w-3 h-3" />
      youtube.com/watch?v={broadcastId}
    </a>
  );
}

function RtmpUrl({ url }: { url: string }) {
  const display = displayUrl(url);
  return (
    <div className="flex items-center gap-2">
      <code className="text-on-surface-variant text-xs font-mono truncate flex-1">
        {display}
      </code>
      <button
        className="text-on-surface-variant hover:text-primary transition-colors p-0.5"
        onClick={() => navigator.clipboard.writeText(display)}
      >
        <Copy className="w-3 h-3" />
      </button>
    </div>
  );
}

function LocalTestLinks({ rtmpUrl }: { rtmpUrl: string }) {
  const path = streamPath(rtmpUrl);
  return (
    <>
      <a
        href={`http://localhost:8889${path}/`}
        target="_blank"
        rel="noopener noreferrer"
        className="flex items-center gap-1.5 text-primary text-xs font-label hover:underline"
      >
        <ExternalLink className="w-3 h-3" />
        WebRTC: localhost:8889{path}/
      </a>
      <a
        href={`http://localhost:8888${path}/`}
        target="_blank"
        rel="noopener noreferrer"
        className="flex items-center gap-1.5 text-primary text-xs font-label hover:underline"
      >
        <ExternalLink className="w-3 h-3" />
        HLS: localhost:8888{path}/
      </a>
    </>
  );
}
