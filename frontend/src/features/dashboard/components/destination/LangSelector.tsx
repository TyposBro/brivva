import { ChevronDown } from "lucide-react";
import { LANGS, langFlag, langLabel } from "../../../../shared/platforms";
import type { Destination } from "../../types";

type Props = {
  dest: Destination;
  fixedLang: string | null;
  sourceLang: string;
  onUpdate: (patch: Partial<Destination>) => void;
};

export function LangSelector({ dest, fixedLang, sourceLang, onUpdate }: Props) {
  if (fixedLang) {
    return (
      <span className="text-[10px] font-label font-bold uppercase tracking-widest text-primary bg-primary/10 px-2 py-0.5 rounded">
        {langFlag(dest.lang)} {langLabel(dest.lang)}
      </span>
    );
  }

  return (
    <div className="relative">
      <select
        className="appearance-none bg-surface-container-highest border-none rounded px-2.5 py-1 text-on-surface font-label text-xs focus:ring-2 focus:ring-primary/50 outline-none pr-6 cursor-pointer"
        value={dest.lang}
        onChange={(e) => onUpdate({ lang: e.target.value })}
      >
        {LANGS.filter((l) => l.code !== sourceLang).map((l) => (
          <option key={l.code} value={l.code}>
            {l.flag} {l.label}
          </option>
        ))}
      </select>
      <ChevronDown className="absolute right-1.5 top-1/2 -translate-y-1/2 w-3 h-3 text-on-surface-variant pointer-events-none" />
    </div>
  );
}
