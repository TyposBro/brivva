import { LANGS, type VoiceMode } from "../../domain/broadcast-types";

type Props = {
  sourceLang: string;
  targetLangs: string[];
  voiceConfig: Record<string, VoiceMode>;
  isLive: boolean;
  onSourceChange: (code: string) => void;
  onTargetToggle: (code: string) => void;
  onVoiceModeChange: (code: string, mode: VoiceMode) => void;
};

const VOICE_OPTIONS: { mode: VoiceMode; label: string; short: string }[] = [
  { mode: "cloned",         label: "Cloned Voice",  short: "Cloned" },
  { mode: "default-female", label: "Default Female", short: "Female" },
  { mode: "default-male",   label: "Default Male",   short: "Male" },
];

export function LanguageConfig({ sourceLang, targetLangs, voiceConfig, isLive, onSourceChange, onTargetToggle, onVoiceModeChange }: Props) {
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
        <div className="flex flex-col gap-2">
          {availableTargets.map((l) => {
            const isSelected = targetLangs.includes(l.code);
            const currentMode = voiceConfig[l.code] ?? "cloned";
            return (
              <div key={l.code} className="flex items-center gap-2">
                <button
                  disabled={isLive}
                  onClick={() => onTargetToggle(l.code)}
                  className={`w-28 px-3 py-1.5 rounded-lg text-sm transition-colors shrink-0 ${
                    isSelected
                      ? "bg-secondary-container text-on-secondary-container font-semibold"
                      : "bg-surface-container text-on-surface-variant hover:bg-surface-container-high"
                  }`}
                >
                  {l.flag} {l.label}
                </button>
                {isSelected && (
                  <div className="flex gap-1">
                    {VOICE_OPTIONS.map((opt) => (
                      <button
                        key={opt.mode}
                        disabled={isLive}
                        onClick={() => onVoiceModeChange(l.code, opt.mode)}
                        title={opt.label}
                        className={`px-2.5 py-1 rounded text-xs font-medium transition-colors ${
                          currentMode === opt.mode
                            ? opt.mode === "cloned"
                              ? "bg-tertiary text-on-tertiary"
                              : "bg-primary text-on-primary"
                            : "bg-surface-container text-on-surface-variant hover:bg-surface-container-high"
                        }`}
                      >
                        {opt.short}
                      </button>
                    ))}
                  </div>
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
