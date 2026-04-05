import { ExternalLink } from "lucide-react";

export function YouTubeLink({ broadcastId }: { broadcastId: string }) {
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
