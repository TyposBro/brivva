import { ArrowLeft } from "lucide-react";

type Props = {
  label?: string;
  onClick: () => void;
};

export function BackButton({ label = "Back", onClick }: Props) {
  return (
    <button
      className="flex items-center gap-2 text-on-surface-variant hover:text-on-surface transition-colors font-label text-sm"
      onClick={onClick}
    >
      <ArrowLeft className="w-4 h-4" />
      {label}
    </button>
  );
}
