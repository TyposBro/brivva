import { useState, useRef } from "react";
import { Plus, Clipboard, ChevronDown, ChevronRight } from "lucide-react";
import { cn } from "../../../lib/cn";
import { PLATFORMS, PLATFORM_LANG, REGION_GROUPS, langFlag, langLabel } from "../../../shared/platforms";
import { PlatformIcon } from "../../../shared/components/PlatformIcon";
import { useOutsideClick } from "../../../shared/hooks/useOutsideClick";

type Props = {
  sourceLang: string;
  magicPaste: string;
  onAddDestination: (platformId: string) => void;
  onMagicPaste: (value: string) => void;
};

export function PlatformPicker({
  sourceLang,
  magicPaste,
  onAddDestination,
  onMagicPaste,
}: Props) {
  const [open, setOpen] = useState(false);
  const [expandedRegion, setExpandedRegion] = useState<string | null>(null);
  const pickerRef = useRef<HTMLDivElement>(null);

  useOutsideClick(pickerRef, () => setOpen(false), open);

  function handleAdd(platformId: string) {
    onAddDestination(platformId);
    setOpen(false);
    setExpandedRegion(null);
  }

  function handleClose() {
    setOpen(false);
    setExpandedRegion(null);
  }

  if (!open) {
    return (
      <ClosedButtons
        onOpen={() => setOpen(true)}
        onPaste={onMagicPaste}
      />
    );
  }

  return (
    <div ref={pickerRef} className="bg-surface-container-low rounded-xl overflow-hidden">
      <PasteBar value={magicPaste} onChange={onMagicPaste} />

      {REGION_GROUPS.map((group) => (
        <RegionGroup
          key={group.key}
          group={group}
          sourceLang={sourceLang}
          expanded={expandedRegion === group.key}
          onToggle={() =>
            setExpandedRegion(expandedRegion === group.key ? null : group.key)
          }
          onSelect={handleAdd}
        />
      ))}

      <button
        className="w-full py-2.5 text-on-surface-variant text-xs font-label hover:bg-surface-container-high transition-colors border-t border-outline-variant/10"
        onClick={handleClose}
      >
        Cancel
      </button>
    </div>
  );
}

function ClosedButtons({
  onOpen,
  onPaste,
}: {
  onOpen: () => void;
  onPaste: (value: string) => void;
}) {
  return (
    <div className="flex gap-2">
      <button
        className="flex-1 flex items-center justify-center gap-2 py-3 rounded-xl border-2 border-dashed border-outline-variant/20 text-on-surface-variant hover:border-primary/40 hover:text-primary transition-all font-label text-sm"
        onClick={onOpen}
      >
        <Plus className="w-4 h-4" />
        Add destination
      </button>
      <button
        className="flex items-center gap-2 px-4 py-3 rounded-xl border-2 border-dashed border-outline-variant/20 text-on-surface-variant hover:border-primary/40 hover:text-primary transition-all"
        onClick={() => {
          const url = prompt("Paste RTMP URL");
          if (url) onPaste(url);
        }}
        title="Paste RTMP URL"
      >
        <Clipboard className="w-4 h-4" />
      </button>
    </div>
  );
}

function PasteBar({ value, onChange }: { value: string; onChange: (v: string) => void }) {
  return (
    <div className="p-3 border-b border-outline-variant/10">
      <input
        className="w-full bg-surface-container-highest border-none rounded-lg px-3 py-2 text-on-surface placeholder:text-on-surface-variant/40 focus:ring-2 focus:ring-primary/50 transition-all font-label text-sm outline-none"
        placeholder="Paste RTMP URL to auto-detect..."
        value={value}
        onChange={(e) => onChange(e.target.value)}
        autoFocus
      />
    </div>
  );
}

function RegionGroup({
  group,
  sourceLang,
  expanded,
  onToggle,
  onSelect,
}: {
  group: (typeof REGION_GROUPS)[number];
  sourceLang: string;
  expanded: boolean;
  onToggle: () => void;
  onSelect: (platformId: string) => void;
}) {
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
