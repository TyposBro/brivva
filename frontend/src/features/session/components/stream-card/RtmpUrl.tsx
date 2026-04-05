import { Copy } from "lucide-react";
import { displayUrl } from "../../../../shared/helpers/rtmp";

export function RtmpUrl({ url }: { url: string }) {
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
