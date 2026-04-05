import { useState } from "react";
import type { BroadcastConfig } from "../hooks/useBroadcastConfig";
import { DELAY_MIN, DELAY_MAX, DELAY_STEP } from "../constants";

type Props = {
  config: BroadcastConfig;
  devices: MediaDeviceInfo[];
  isLive: boolean;
  onConfigChange: <K extends keyof BroadcastConfig>(key: K, value: BroadcastConfig[K]) => void;
};

export function BroadcastSettings({ config, devices, isLive, onConfigChange }: Props) {
  const [open, setOpen] = useState(false);

  return (
    <div className="bg-surface-container-low rounded-xl overflow-hidden">
      <button
        onClick={() => setOpen((p) => !p)}
        className="w-full px-4 py-3 flex items-center justify-between text-sm font-semibold text-on-surface-variant uppercase tracking-wider hover:bg-surface-container transition-colors"
      >
        Settings
        <span className="text-xs text-outline">{open ? "Hide" : "Show"}</span>
      </button>
      {open && (
        <div className="px-4 pb-4 space-y-4 border-t border-outline-variant">
          <DelaySlider value={config.broadcastDelay} disabled={isLive} onChange={(v) => onConfigChange("broadcastDelay", v)} />
          <DeviceSelect label="Camera" kind="videoinput" devices={devices} value={config.videoDeviceId} disabled={isLive} onChange={(v) => onConfigChange("videoDeviceId", v)} />
          <DeviceSelect label="Microphone" kind="audioinput" devices={devices} value={config.audioDeviceId} disabled={isLive} onChange={(v) => onConfigChange("audioDeviceId", v)} />
          <TtsModelPicker value={config.ttsModel} disabled={isLive} onChange={(v) => onConfigChange("ttsModel", v)} />
        </div>
      )}
    </div>
  );
}

function DelaySlider({ value, disabled, onChange }: { value: number; disabled: boolean; onChange: (v: number) => void }) {
  return (
    <div className="pt-3 space-y-1">
      <label className="text-sm text-on-surface-variant">Broadcast Delay: {(value / 1000).toFixed(1)}s</label>
      <input type="range" min={DELAY_MIN} max={DELAY_MAX} step={DELAY_STEP} disabled={disabled} value={value} onChange={(e) => onChange(Number(e.target.value))} className="w-full accent-primary" />
      <p className="text-xs text-outline">Higher = more time for TTS, lower = less stream latency</p>
    </div>
  );
}

function DeviceSelect({ label, kind, devices, value, disabled, onChange }: { label: string; kind: string; devices: MediaDeviceInfo[]; value: string; disabled: boolean; onChange: (v: string) => void }) {
  const filtered = devices.filter((d) => d.kind === kind);
  const fallback = kind === "videoinput" ? "Camera" : "Mic";
  return (
    <div className="space-y-1">
      <label className="text-sm text-on-surface-variant">{label}</label>
      <select disabled={disabled} value={value} onChange={(e) => onChange(e.target.value)} className="w-full px-3 py-2 rounded-lg bg-surface-container border border-outline-variant text-sm text-on-surface disabled:opacity-50">
        <option value="">Default</option>
        {filtered.map((d) => (
          <option key={d.deviceId} value={d.deviceId}>{d.label || `${fallback} ${d.deviceId.slice(0, 8)}`}</option>
        ))}
      </select>
    </div>
  );
}

function TtsModelPicker({ value, disabled, onChange }: { value: "turbo" | "flash"; disabled: boolean; onChange: (v: "turbo" | "flash") => void }) {
  const btn = (model: "turbo" | "flash", label: string, latency: string) => (
    <button
      disabled={disabled}
      onClick={() => onChange(model)}
      className={`flex-1 px-3 py-2 rounded-lg text-sm font-medium transition-colors disabled:opacity-50 ${
        value === model ? "bg-primary text-on-primary" : "bg-surface-container border border-outline-variant text-on-surface-variant hover:bg-surface-container-high"
      }`}
    >
      {label}<span className="block text-xs opacity-70">{latency}</span>
    </button>
  );

  return (
    <div className="space-y-1">
      <label className="text-sm text-on-surface-variant">TTS Model</label>
      <div className="flex gap-2">
        {btn("turbo", "Expressive", "~300ms")}
        {btn("flash", "Fast", "~75ms")}
      </div>
    </div>
  );
}
