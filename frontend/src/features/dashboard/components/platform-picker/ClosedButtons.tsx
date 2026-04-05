import { Plus, Clipboard } from "lucide-react";

type Props = {
  onOpen: () => void;
  onPaste: (value: string) => void;
};

export function ClosedButtons({ onOpen, onPaste }: Props) {
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
