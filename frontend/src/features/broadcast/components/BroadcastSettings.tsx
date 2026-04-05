import { useState } from "react";
import type { BroadcastConfig } from "../hooks/useBroadcastConfig";
import { DelaySlider } from "./settings/DelaySlider";
import { DeviceSelect } from "./settings/DeviceSelect";
import { TtsModelPicker } from "./settings/TtsModelPicker";

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
          <DelaySlider
            value={config.broadcastDelay}
            disabled={isLive}
            onChange={(v) => onConfigChange("broadcastDelay", v)}
          />
          <DeviceSelect
            label="Camera"
            kind="videoinput"
            devices={devices}
            value={config.videoDeviceId}
            disabled={isLive}
            onChange={(v) => onConfigChange("videoDeviceId", v)}
          />
          <DeviceSelect
            label="Microphone"
            kind="audioinput"
            devices={devices}
            value={config.audioDeviceId}
            disabled={isLive}
            onChange={(v) => onConfigChange("audioDeviceId", v)}
          />
          <TtsModelPicker
            value={config.ttsModel}
            disabled={isLive}
            onChange={(v) => onConfigChange("ttsModel", v)}
          />
        </div>
      )}
    </div>
  );
}
