import { LANGS } from "../constants";

type Props = {
  sourceLang: string;
  targetLangs: string[];
  isLive: boolean;
  onSourceChange: (code: string) => void;
  onTargetToggle: (code: string) => void;
};

export function LanguageConfig({ sourceLang, targetLangs, isLive, onSourceChange, onTargetToggle }: Props) {
  const availableTargets = LANGS.filter((l) => l.code !== sourceLang);
  const selectedCount = targetLangs.filter((l) => l !== sourceLang).length;
  const sourceName = LANGS.find((l) => l.code === sourceLang)?.label;

  return (
    <div className="bg-surface-container-low rounded-xl p-4 space-y-4">
      <div>
        <h2 className="font-headline text-sm font-semibold text-on-surface-variant uppercase tracking-wider mb-2">
          Host Language
        </h2>
        <div className="flex gap-2">
          {LANGS.map((l) => (
            <button
              key={l.code}
              disabled={isLive}
              onClick={() => onSourceChange(l.code)}
              className={`px-3 py-1.5 rounded-lg text-sm transition-colors ${
                sourceLang === l.code
                  ? "bg-primary-container text-on-primary-container font-semibold"
                  : "bg-surface-container text-on-surface-variant hover:bg-surface-container-high"
              }`}
            >
              {l.flag} {l.label}
            </button>
          ))}
        </div>
      </div>

      <div>
        <h2 className="font-headline text-sm font-semibold text-on-surface-variant uppercase tracking-wider mb-2">
          Translate To
        </h2>
        <div className="flex gap-2">
          {availableTargets.map((l) => (
            <button
              key={l.code}
              disabled={isLive}
              onClick={() => onTargetToggle(l.code)}
              className={`px-3 py-1.5 rounded-lg text-sm transition-colors ${
                targetLangs.includes(l.code)
                  ? "bg-secondary-container text-on-secondary-container font-semibold"
                  : "bg-surface-container text-on-surface-variant hover:bg-surface-container-high"
              }`}
            >
              {l.flag} {l.label}
            </button>
          ))}
        </div>
        <p className="text-xs text-outline mt-2">
          {selectedCount} language(s) selected
          {sourceLang && ` + 1 passthrough (${sourceName})`}
        </p>
      </div>
    </div>
  );
}
