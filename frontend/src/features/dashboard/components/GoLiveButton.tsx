import { Radio } from "lucide-react";
import { cn } from "../../../lib/cn";

type Props = {
  destinationCount: number;
  creating: boolean;
  onClick: () => void;
};

export function GoLiveButton({ destinationCount, creating, onClick }: Props) {
  const enabled = destinationCount > 0;

  return (
    <button
      className={cn(
        "w-full py-4 rounded-xl font-headline font-extrabold text-lg uppercase tracking-tight transition-all",
        enabled
          ? "monolith-gradient text-white hover:scale-[0.99] active:scale-[0.97] shadow-xl"
          : "bg-surface-container-high text-on-surface-variant cursor-not-allowed",
      )}
      onClick={onClick}
      disabled={creating || !enabled}
    >
      <span className="flex items-center justify-center gap-2">
        <Radio className="w-5 h-5" />
        {buttonLabel(destinationCount, creating)}
      </span>
    </button>
  );
}

function buttonLabel(count: number, creating: boolean): string {
  if (creating) return "Creating...";
  if (count === 0) return "Add a destination to go live";
  if (count === 1) return "Go Live";
  return `Go Live \u00B7 ${count} destinations`;
}
