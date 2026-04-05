import { ExternalLink } from "lucide-react";
import { streamPath } from "../../../../shared/helpers/rtmp";

export function LocalTestLinks({ rtmpUrl }: { rtmpUrl: string }) {
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
