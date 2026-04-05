import { useState } from "react";
import { PLATFORMS } from "../../../shared/platforms";
import type { PlatformCredential } from "../../../shared/api";
import type { Destination } from "../types";

import { CardHeader } from "./destination/CardHeader";
import { ConfigSection } from "./destination/ConfigSection";

type Props = {
  dest: Destination;
  sourceLang: string;
  savedCreds: Record<string, PlatformCredential>;
  onUpdate: (patch: Partial<Destination>) => void;
  onRemove: () => void;
};

export function DestinationCard({ dest, sourceLang, savedCreds, onUpdate, onRemove }: Props) {
  const platform = PLATFORMS.find((p) => p.id === dest.platform);
  if (!platform) return null;

  const needsConfig = !platform.auto;
  const hasSavedCreds = !!savedCreds[dest.platform];
  const [expanded, setExpanded] = useState(() => !!(needsConfig && !hasSavedCreds));

  return (
    <div className="bg-surface-container-low rounded-xl overflow-hidden">
      <CardHeader
        dest={dest}
        platform={platform}
        sourceLang={sourceLang}
        hasSavedCreds={hasSavedCreds}
        expanded={expanded}
        onToggle={() => setExpanded(!expanded)}
        onUpdate={onUpdate}
        onRemove={onRemove}
      />
      {needsConfig && expanded && (
        <ConfigSection
          dest={dest}
          platform={platform}
          hasSavedCreds={hasSavedCreds}
          onUpdate={onUpdate}
        />
      )}
    </div>
  );
}
