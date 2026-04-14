import { LANGS } from "../../domain/broadcast-types";

type Props = {
  sourceLang: string;
  targetLangs: string[];
  voiceDefaultLangs: string[];
  isLive: boolean;
  onSourceChange: (code: string) => void;
  onTargetToggle: (code: string) => void;
  onVoiceDefaultToggle: (code: string) => void;
};

export function LanguageConfig({ sourceLang, targetLangs, voiceDefaultLangs, isLive, onSourceChange, onTargetToggle, onVoiceDefaultToggle }: Props) {
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
        <div className="flex flex-wrap gap-2">
          {availableTargets.map((l) => {
            const isSelected = targetLangs.includes(l.code);
            const isDefaultVoice = voiceDefaultLangs.includes(l.code);
            return (
              <div key={l.code} className="flex flex-col gap-1">
                <button
                  disabled={isLive}
                  onClick={() => onTargetToggle(l.code)}
                  className={`px-3 py-1.5 rounded-lg text-sm transition-colors ${
                    isSelected
                      ? "bg-secondary-container text-on-secondary-container font-semibold"
                      : "bg-surface-container text-on-surface-variant hover:bg-surface-container-high"
                  }`}
                >
                  {l.flag} {l.label}
                </button>
                {isSelected && (
                  <button
                    disabled={isLive}
                    onClick={() => onVoiceDefaultToggle(l.code)}
                    title={isDefaultVoice ? "Using default voice — click to use clone" : "Using cloned voice — click to use default"}
                    className={`px-2 py-0.5 rounded text-xs transition-colors ${
                      isDefaultVoice
                        ? "bg-tertiary-container text-on-tertiary-container"
                        : "bg-surface-container-high text-on-surface-variant hover:bg-surface-container-highest"
                    }`}
                  >
                    {isDefaultVoice ? "default voice" : "clone"}
                  </button>
                )}
              </div>
            );
          })}
        </div>
        <p className="text-xs text-outline mt-2">
          {selectedCount} language(s) selected
          {sourceLang && ` + 1 passthrough (${sourceName})`}
        </p>
      </div>
    </div>
  );
}
