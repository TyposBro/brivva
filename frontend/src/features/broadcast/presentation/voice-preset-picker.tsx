import { Check, Mic, User, UserRound } from "lucide-react";
import { cn } from "../../../core/cn";
import type { VoicePreset } from "../data/api-client";

type Option = {
  id: VoicePreset;
  label: string;
  subtitle: string;
  icon: React.ComponentType<{ className?: string }>;
  disabled?: boolean;
};

type Props = {
  value: VoicePreset;
  onChange: (preset: VoicePreset) => void;
  hasClone: boolean;
  onReRecord: () => void;
};

export function VoicePresetPicker({ value, onChange, hasClone, onReRecord }: Props) {
  const options: Option[] = [
    {
      id: "cloned",
      label: hasClone ? "Your voice" : "No cloned voice",
      subtitle: hasClone
        ? "Use the clone you recorded earlier"
        : "Record one below to unlock",
      icon: Mic,
      disabled: !hasClone,
    },
    { id: "female", label: "Default female", subtitle: "Library voice, per language", icon: UserRound },
    { id: "male", label: "Default male", subtitle: "Library voice, per language", icon: User },
  ];

  return (
    <section className="bg-surface-container-low rounded-xl p-6 max-w-lg mx-auto space-y-4">
      <header>
        <h3 className="font-headline font-bold text-lg text-on-surface">Voice</h3>
        <p className="text-on-surface-variant text-sm font-label">
          Pick which voice TTS should use for translated audio.
        </p>
      </header>
      <ul className="space-y-2">
        {options.map((opt) => {
          const selected = value === opt.id;
          const Icon = opt.icon;
          return (
            <li key={opt.id}>
              <button
                type="button"
                disabled={opt.disabled}
                onClick={() => !opt.disabled && onChange(opt.id)}
                className={cn(
                  "w-full flex items-center gap-3 px-4 py-3 rounded-lg text-left transition-colors",
                  opt.disabled
                    ? "bg-surface-container text-on-surface-variant/60 cursor-not-allowed"
                    : selected
                      ? "bg-primary/15 text-on-surface ring-1 ring-primary"
                      : "bg-surface-container-high hover:bg-surface-bright text-on-surface",
                )}
              >
                <Icon className="w-5 h-5 shrink-0" />
                <div className="flex-1 min-w-0">
                  <div className="font-label font-semibold text-sm">{opt.label}</div>
                  <div className="text-on-surface-variant text-xs font-label">{opt.subtitle}</div>
                </div>
                {selected && !opt.disabled && <Check className="w-5 h-5 text-primary" />}
              </button>
            </li>
          );
        })}
      </ul>
      {hasClone && (
        <button
          type="button"
          onClick={onReRecord}
          className="text-on-surface-variant hover:text-on-surface text-sm font-label underline decoration-dotted underline-offset-2"
        >
          Re-record voice sample
        </button>
      )}
    </section>
  );
}
