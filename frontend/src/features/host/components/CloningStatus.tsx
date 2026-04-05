import { Loader2 } from "lucide-react";

export function CloningStatus() {
  return (
    <div className="flex items-center justify-center gap-3 text-on-surface-variant font-label py-12">
      <Loader2 className="w-5 h-5 animate-spin text-primary" />
      Cloning your voice...
    </div>
  );
}
