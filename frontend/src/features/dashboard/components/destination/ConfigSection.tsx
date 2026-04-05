import { ExternalLink } from "lucide-react";
import type { Platform } from "../../../../shared/platforms";
import type { Destination } from "../../types";

type Props = {
  dest: Destination;
  platform: Platform;
  hasSavedCreds: boolean;
  onUpdate: (patch: Partial<Destination>) => void;
};

export function ConfigSection({ dest, platform, hasSavedCreds, onUpdate }: Props) {
  return (
    <div className="px-4 pb-4 space-y-2">
      {hasSavedCreds && (
        <span className="text-[10px] font-label text-success uppercase tracking-widest">
          Pre-filled from saved credentials
        </span>
      )}
      {platform.settingsUrl && (
        <a
          href={platform.settingsUrl}
          target="_blank"
          rel="noopener noreferrer"
          className="flex items-center gap-1 text-primary text-xs font-label hover:underline"
        >
          Open {platform.label} Settings
          <ExternalLink className="w-3 h-3" />
        </a>
      )}
      <p className="text-on-surface-variant/60 text-xs leading-relaxed">
        {platform.help}
      </p>
      <div className="grid grid-cols-1 sm:grid-cols-2 gap-2">
        {!platform.keyOnly && (
          <input
            className="bg-surface-container-highest border-none rounded-lg px-3 py-2 text-on-surface placeholder:text-on-surface-variant/50 focus:ring-2 focus:ring-primary/50 transition-all font-label text-sm outline-none"
            placeholder="Server URL"
            value={dest.rtmp_url}
            onChange={(e) => onUpdate({ rtmp_url: e.target.value })}
          />
        )}
        <input
          className="bg-surface-container-highest border-none rounded-lg px-3 py-2 text-on-surface placeholder:text-on-surface-variant/50 focus:ring-2 focus:ring-primary/50 transition-all font-label text-sm outline-none"
          placeholder="Stream Key"
          type="password"
          value={dest.stream_key}
          onChange={(e) => onUpdate({ stream_key: e.target.value })}
        />
      </div>
    </div>
  );
}
