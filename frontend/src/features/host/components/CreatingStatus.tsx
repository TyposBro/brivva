import { Loader2 } from "lucide-react";

export function CreatingStatus() {
  return (
    <div className="flex items-center gap-3 text-on-surface-variant font-label">
      <Loader2 className="w-4 h-4 animate-spin" />
      Creating room...
    </div>
  );
}
