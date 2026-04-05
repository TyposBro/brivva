import { X } from "lucide-react";
import { PLATFORM_LANG } from "../../../../shared/platforms";
import { PlatformIcon } from "../../../../shared/components/PlatformIcon";
import type { Platform } from "../../../../shared/platforms";
import type { Destination } from "../../types";

import { LangSelector } from "./LangSelector";
import { ConfigIndicator } from "./ConfigIndicator";
import { ExpandToggle } from "./ExpandToggle";

type Props = {
  dest: Destination;
  platform: Platform;
  sourceLang: string;
  hasSavedCreds: boolean;
  expanded: boolean;
  onToggle: () => void;
  onUpdate: (patch: Partial<Destination>) => void;
  onRemove: () => void;
};

export function CardHeader({
  dest,
  platform,
  sourceLang,
  hasSavedCreds,
  expanded,
  onToggle,
  onUpdate,
  onRemove,
}: Props) {
  const fixedLang = PLATFORM_LANG[dest.platform];
  const needsConfig = !platform.auto;
  const hasConfig = !!(dest.rtmp_url || dest.stream_key);

  return (
    <div className="flex items-center gap-3 px-4 py-3">
      <PlatformIcon id={dest.platform} className="w-4 h-4 text-primary" />
      <span className="font-label font-bold text-on-surface text-sm flex-1 truncate">
        {platform.label}
      </span>

      <LangSelector dest={dest} fixedLang={fixedLang} sourceLang={sourceLang} onUpdate={onUpdate} />

      {needsConfig && <ConfigIndicator configured={hasConfig || hasSavedCreds} />}
      {needsConfig && <ExpandToggle expanded={expanded} onToggle={onToggle} />}

      <button
        className="text-on-surface-variant hover:text-error transition-colors p-1"
        onClick={onRemove}
      >
        <X className="w-3.5 h-3.5" />
      </button>
    </div>
  );
}
