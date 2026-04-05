import { cn } from "../../../../lib/cn";

export function ConfigIndicator({ configured }: { configured: boolean }) {
  return (
    <span
      className={cn(
        "w-2 h-2 rounded-full shrink-0",
        configured ? "bg-success" : "bg-error/60",
      )}
      title={configured ? "Configured" : "Needs stream key"}
    />
  );
}
