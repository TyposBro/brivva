import { ChevronDown, Youtube } from "lucide-react";

type Props = {
  value: string;
  onChange: (v: string) => void;
};

export function YouTubePrivacy({ value, onChange }: Props) {
  return (
    <div className="flex items-center gap-3 bg-surface-container-low rounded-xl px-4 py-3">
      <Youtube className="w-4 h-4 text-[#ff0000]" />
      <span className="text-on-surface-variant text-xs font-label flex-1">
        YouTube privacy
      </span>
      <div className="relative">
        <select
          className="appearance-none bg-surface-container-highest border-none rounded-lg px-3 py-1.5 text-on-surface font-label text-sm focus:ring-2 focus:ring-primary/50 outline-none pr-7 cursor-pointer"
          value={value}
          onChange={(e) => onChange(e.target.value)}
        >
          <option value="public">Public</option>
          <option value="unlisted">Unlisted</option>
          <option value="private">Private</option>
        </select>
        <ChevronDown className="absolute right-2 top-1/2 -translate-y-1/2 w-3 h-3 text-on-surface-variant pointer-events-none" />
      </div>
    </div>
  );
}
