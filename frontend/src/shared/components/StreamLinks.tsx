import { ExternalLink } from "lucide-react";
import { streamPath } from "../helpers/rtmp";

export function StreamLinks({ rtmpUrl }: { rtmpUrl: string }) {
  const path = streamPath(rtmpUrl);
  return (
    <div className="flex flex-col gap-0.5">
      <StreamLink href={`http://localhost:8889${path}/`} label="WebRTC" />
      <StreamLink href={`http://localhost:8888${path}/`} label="HLS" />
    </div>
  );
}

function StreamLink({ href, label }: { href: string; label: string }) {
  return (
    <a
      href={href}
      target="_blank"
      rel="noopener noreferrer"
      className="flex items-center gap-1 text-primary text-[10px] font-label hover:underline"
    >
      <ExternalLink className="w-2.5 h-2.5" />
      {label}
    </a>
  );
}
