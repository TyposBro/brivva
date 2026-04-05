import { useState } from "react";
import { X, ChevronDown, ChevronRight, ExternalLink } from "lucide-react";
import { cn } from "../../../lib/cn";
import { PLATFORMS, PLATFORM_LANG, LANGS, langFlag, langLabel } from "../../../shared/platforms";
import { PlatformIcon } from "../../../shared/components/PlatformIcon";
import type { PlatformCredential } from "../../../shared/api";
import type { Destination } from "../types";

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

  const fixedLang = PLATFORM_LANG[dest.platform];
  const needsConfig = !platform.auto;
  const hasConfig = !!(dest.rtmp_url || dest.stream_key);
  const hasSavedCreds = !!savedCreds[dest.platform];

  const [expanded, setExpanded] = useState(() => !!(needsConfig && !hasSavedCreds));

  return (
    <div className="bg-surface-container-low rounded-xl overflow-hidden">
      <CardHeader
        dest={dest}
        platformLabel={platform.label}
        fixedLang={fixedLang}
        sourceLang={sourceLang}
        needsConfig={needsConfig}
        hasConfig={hasConfig}
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

function CardHeader({
  dest,
  platformLabel,
  fixedLang,
  sourceLang,
  needsConfig,
  hasConfig,
  hasSavedCreds,
  expanded,
  onToggle,
  onUpdate,
  onRemove,
}: {
  dest: Destination;
  platformLabel: string;
  fixedLang: string | null;
  sourceLang: string;
  needsConfig: boolean;
  hasConfig: boolean;
  hasSavedCreds: boolean;
  expanded: boolean;
  onToggle: () => void;
  onUpdate: (patch: Partial<Destination>) => void;
  onRemove: () => void;
}) {
  return (
    <div className="flex items-center gap-3 px-4 py-3">
      <PlatformIcon id={dest.platform} className="w-4 h-4 text-primary" />
      <span className="font-label font-bold text-on-surface text-sm flex-1 truncate">
        {platformLabel}
      </span>

      <LangSelector
        dest={dest}
        fixedLang={fixedLang}
        sourceLang={sourceLang}
        onUpdate={onUpdate}
      />

      {needsConfig && (
        <span
          className={cn(
            "w-2 h-2 rounded-full shrink-0",
            hasConfig || hasSavedCreds ? "bg-success" : "bg-error/60",
          )}
          title={hasConfig || hasSavedCreds ? "Configured" : "Needs stream key"}
        />
      )}

      {needsConfig && (
        <button
          className="text-on-surface-variant hover:text-on-surface p-1 transition-colors"
          onClick={onToggle}
        >
          {expanded ? (
            <ChevronDown className="w-3.5 h-3.5" />
          ) : (
            <ChevronRight className="w-3.5 h-3.5" />
          )}
        </button>
      )}

      <button
        className="text-on-surface-variant hover:text-error transition-colors p-1"
        onClick={onRemove}
      >
        <X className="w-3.5 h-3.5" />
      </button>
    </div>
  );
}

function LangSelector({
  dest,
  fixedLang,
  sourceLang,
  onUpdate,
}: {
  dest: Destination;
  fixedLang: string | null;
  sourceLang: string;
  onUpdate: (patch: Partial<Destination>) => void;
}) {
  if (fixedLang) {
    return (
      <span className="text-[10px] font-label font-bold uppercase tracking-widest text-primary bg-primary/10 px-2 py-0.5 rounded">
        {langFlag(dest.lang)} {langLabel(dest.lang)}
      </span>
    );
  }

  return (
    <div className="relative">
      <select
        className="appearance-none bg-surface-container-highest border-none rounded px-2.5 py-1 text-on-surface font-label text-xs focus:ring-2 focus:ring-primary/50 outline-none pr-6 cursor-pointer"
        value={dest.lang}
        onChange={(e) => onUpdate({ lang: e.target.value })}
      >
        {LANGS.filter((l) => l.code !== sourceLang).map((l) => (
          <option key={l.code} value={l.code}>
            {l.flag} {l.label}
          </option>
        ))}
      </select>
      <ChevronDown className="absolute right-1.5 top-1/2 -translate-y-1/2 w-3 h-3 text-on-surface-variant pointer-events-none" />
    </div>
  );
}

function ConfigSection({
  dest,
  platform,
  hasSavedCreds,
  onUpdate,
}: {
  dest: Destination;
  platform: { label: string; settingsUrl: string; help: string; keyOnly: boolean };
  hasSavedCreds: boolean;
  onUpdate: (patch: Partial<Destination>) => void;
}) {
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
