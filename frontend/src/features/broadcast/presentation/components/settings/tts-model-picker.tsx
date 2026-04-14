import type { TtsProvider } from "../../hooks/use-broadcast-config";

type TtsModel = "turbo" | "flash";
type VoiceGender = "female" | "male";

type Props = {
  value: TtsModel;
  provider: TtsProvider;
  voiceGender: VoiceGender;
  disabled: boolean;
  onChange: (v: TtsModel) => void;
  onProviderChange: (v: TtsProvider) => void;
  onVoiceGenderChange: (v: VoiceGender) => void;
};

const PROVIDERS: { id: TtsProvider; label: string; desc: string }[] = [
  { id: "elevenlabs", label: "ElevenLabs", desc: "Current provider" },
  { id: "dashscope", label: "Qwen3 TTS", desc: "Higher clone quality" },
];

const MODELS: { id: TtsModel; label: string; latency: string }[] = [
  { id: "turbo", label: "Expressive", latency: "~300ms" },
  { id: "flash", label: "Fast", latency: "~75ms" },
];

export function TtsModelPicker({ value, provider, voiceGender, disabled, onChange, onProviderChange, onVoiceGenderChange }: Props) {
  const isDashscope = provider === "dashscope";
  const genders: { id: VoiceGender; label: string; desc: string }[] = [
    { id: "female", label: "Female", desc: isDashscope ? "longxiaochun" : "Rachel" },
    { id: "male",   label: "Male",   desc: isDashscope ? "longman" : "Adam" },
  ];
  return (
    <div className="space-y-3">
      <div className="space-y-1">
        <label className="text-sm text-on-surface-variant">TTS Provider</label>
        <div className="flex gap-2">
          {PROVIDERS.map((p) => (
            <button
              key={p.id}
              disabled={disabled}
              onClick={() => onProviderChange(p.id)}
              className={`flex-1 px-3 py-2 rounded-lg text-sm font-medium transition-colors disabled:opacity-50 ${
                provider === p.id
                  ? "bg-primary text-on-primary"
                  : "bg-surface-container border border-outline-variant text-on-surface-variant hover:bg-surface-container-high"
              }`}
            >
              {p.label}
              <span className="block text-xs opacity-70">{p.desc}</span>
            </button>
          ))}
        </div>
      </div>
      {provider === "elevenlabs" && (
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
      )}
      <div className="space-y-1">
        <label className="text-sm text-on-surface-variant">Default Voice</label>
        <div className="flex gap-2">
          {genders.map((g) => (
            <button
              key={g.id}
              disabled={disabled}
              onClick={() => onVoiceGenderChange(g.id)}
              className={`flex-1 px-3 py-2 rounded-lg text-sm font-medium transition-colors disabled:opacity-50 ${
                voiceGender === g.id
                  ? "bg-primary text-on-primary"
                  : "bg-surface-container border border-outline-variant text-on-surface-variant hover:bg-surface-container-high"
              }`}
            >
              {g.label}
              <span className="block text-xs opacity-70">{g.desc}</span>
            </button>
          ))}
        </div>
      </div>
    </div>
  );
}
