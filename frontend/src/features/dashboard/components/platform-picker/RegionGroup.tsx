import { ChevronDown, ChevronRight } from "lucide-react";
import { cn } from "../../../../lib/cn";
import { PLATFORMS, PLATFORM_LANG, REGION_GROUPS, langFlag, langLabel } from "../../../../shared/platforms";
import { PlatformIcon } from "../../../../shared/components/PlatformIcon";

type Props = {
  group: (typeof REGION_GROUPS)[number];
  sourceLang: string;
  expanded: boolean;
  onToggle: () => void;
  onSelect: (platformId: string) => void;
};

export function RegionGroup({ group, sourceLang, expanded, onToggle, onSelect }: Props) {
  const platforms = PLATFORMS.filter((p) => p.region === group.key);
  if (platforms.length === 0) return null;

  return (
    <div>
      <button
        className="w-full flex items-center gap-3 px-4 py-3 hover:bg-surface-container-high transition-colors text-left"
        onClick={onToggle}
      >
        <span className="text-sm">{group.icon}</span>
        <span className="text-on-surface font-label text-sm flex-1">
          {group.label}
        </span>
        <span className="text-on-surface-variant/40 text-xs font-label">
          {platforms.length}
        </span>
        {expanded ? (
          <ChevronDown className="w-3.5 h-3.5 text-on-surface-variant" />
        ) : (
          <ChevronRight className="w-3.5 h-3.5 text-on-surface-variant" />
        )}
      </button>

      {expanded && (
        <div className="pb-2">
          {platforms.map((p) => {
            const autoLang = PLATFORM_LANG[p.id];
            const disabled = autoLang !== null && autoLang === sourceLang;

            return (
              <button
                key={p.id}
                disabled={disabled}
                className={cn(
                  "w-full flex items-center gap-3 px-4 pl-10 py-2.5 text-left transition-colors",
                  disabled
                    ? "text-on-surface-variant/30 cursor-not-allowed"
                    : "text-on-surface hover:bg-surface-container-high cursor-pointer",
                )}
                onClick={() => onSelect(p.id)}
              >
                <PlatformIcon id={p.id} className="w-4 h-4 shrink-0" />
                <span className="font-label text-sm flex-1">{p.label}</span>
                {autoLang && !disabled && (
                  <span className="text-xs text-on-surface-variant/50">
                    {langFlag(autoLang)} {langLabel(autoLang)}
                  </span>
                )}
                {disabled && (
                  <span className="text-[10px] text-on-surface-variant/30 font-label">
                    same as source
                  </span>
                )}
              </button>
            );
          })}
        </div>
      )}
    </div>
  );
}
