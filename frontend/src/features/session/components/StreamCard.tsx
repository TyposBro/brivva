import type { StreamInfo } from "../../../shared/api";
import { StreamCardHeader } from "./stream-card/StreamCardHeader";
import { YouTubeLink } from "./stream-card/YouTubeLink";
import { RtmpUrl } from "./stream-card/RtmpUrl";
import { LocalTestLinks } from "./stream-card/LocalTestLinks";

export function StreamCard({ stream: s }: { stream: StreamInfo }) {
  return (
    <div className="bg-surface-container-low rounded-xl p-5 space-y-3">
      <StreamCardHeader stream={s} />

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
