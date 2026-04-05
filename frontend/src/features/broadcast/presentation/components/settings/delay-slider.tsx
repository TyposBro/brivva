import { DELAY_MIN, DELAY_MAX, DELAY_STEP } from "../../../domain/broadcast-constants";

const MS_PER_SECOND = 1000;

type Props = {
  value: number;
  disabled: boolean;
  onChange: (v: number) => void;
};

export function DelaySlider({ value, disabled, onChange }: Props) {
  return (
    <div className="pt-3 space-y-1">
      <label className="text-sm text-on-surface-variant">
        Broadcast Delay: {(value / MS_PER_SECOND).toFixed(1)}s
      </label>
      <input
        type="range"
        min={DELAY_MIN}
        max={DELAY_MAX}
        step={DELAY_STEP}
        disabled={disabled}
        value={value}
        onChange={(e) => onChange(Number(e.target.value))}
        className="w-full accent-primary"
      />
      <p className="text-xs text-outline">
        Higher = more time for TTS, lower = less stream latency
      </p>
    </div>
  );
}
