import { ChevronDown } from "lucide-react";
import { LANGS } from "../../../shared/platforms";

type Props = {
  title: string;
  sourceLang: string;
  onTitleChange: (v: string) => void;
  onLangChange: (v: string) => void;
};

export function TitleRow({ title, sourceLang, onTitleChange, onLangChange }: Props) {
  return (
    <div className="flex gap-3">
      <input
        className="flex-1 min-w-0 bg-surface-container-highest border-none rounded-xl px-4 py-3 text-on-surface placeholder:text-on-surface-variant/50 focus:ring-2 focus:ring-primary/50 transition-all font-label outline-none"
        placeholder="Session title"
        value={title}
        onChange={(e) => onTitleChange(e.target.value)}
      />
      <div className="relative shrink-0">
        <select
          className="appearance-none bg-surface-container-highest border-none rounded-xl px-4 py-3 text-on-surface font-label focus:ring-2 focus:ring-primary/50 transition-all outline-none pr-9 cursor-pointer"
          value={sourceLang}
          onChange={(e) => onLangChange(e.target.value)}
        >
          {LANGS.map((l) => (
            <option key={l.code} value={l.code}>
              {l.flag} {l.label}
            </option>
          ))}
        </select>
        <ChevronDown className="absolute right-3 top-1/2 -translate-y-1/2 w-4 h-4 text-on-surface-variant pointer-events-none" />
      </div>
    </div>
  );
}
