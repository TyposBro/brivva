import { ChevronDown, ChevronRight } from "lucide-react";

export function ExpandToggle({ expanded, onToggle }: { expanded: boolean; onToggle: () => void }) {
  const Icon = expanded ? ChevronDown : ChevronRight;
  return (
    <button
      className="text-on-surface-variant hover:text-on-surface p-1 transition-colors"
      onClick={onToggle}
    >
      <Icon className="w-3.5 h-3.5" />
    </button>
  );
}
