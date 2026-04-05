import { useState, useRef } from "react";
import { REGION_GROUPS } from "../../../shared/platforms";
import { useOutsideClick } from "../../../shared/hooks/useOutsideClick";

import { ClosedButtons } from "./platform-picker/ClosedButtons";
import { PasteBar } from "./platform-picker/PasteBar";
import { RegionGroup } from "./platform-picker/RegionGroup";

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
