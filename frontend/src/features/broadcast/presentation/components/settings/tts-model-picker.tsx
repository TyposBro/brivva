type TtsModel = "turbo" | "flash";

type Props = {
  value: TtsModel;
  disabled: boolean;
  onChange: (v: TtsModel) => void;
};

const MODELS: { id: TtsModel; label: string; latency: string }[] = [
  { id: "turbo", label: "Expressive", latency: "~300ms" },
  { id: "flash", label: "Fast", latency: "~75ms" },
];

export function TtsModelPicker({ value, disabled, onChange }: Props) {
  return (
    <div className="space-y-1">
      <label className="text-sm text-on-surface-variant">TTS Model</label>
      <div className="flex gap-2">
        {MODELS.map((model) => (
          <button
            key={model.id}
            disabled={disabled}
            onClick={() => onChange(model.id)}
            className={`flex-1 px-3 py-2 rounded-lg text-sm font-medium transition-colors disabled:opacity-50 ${
              value === model.id
                ? "bg-primary text-on-primary"
                : "bg-surface-container border border-outline-variant text-on-surface-variant hover:bg-surface-container-high"
            }`}
          >
            {model.label}
            <span className="block text-xs opacity-70">{model.latency}</span>
          </button>
        ))}
      </div>
    </div>
  );
}
