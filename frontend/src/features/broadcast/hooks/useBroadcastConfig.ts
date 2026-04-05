import { useState, useEffect } from "react";
import { STORAGE_KEY, type TranslationTier } from "../constants";

export type BroadcastConfig = {
  sourceLang: string;
  targetLangs: string[];
  tier: TranslationTier;
  rtmpUrls: Record<string, string>;
  broadcastDelay: number;
  videoDeviceId: string;
  audioDeviceId: string;
  ttsModel: "turbo" | "flash";
};

function defaultConfig(): BroadcastConfig {
  return {
    sourceLang: "en",
    targetLangs: ["ja", "ko"],
    tier: 2,
    rtmpUrls: {},
    broadcastDelay: 5000,
    videoDeviceId: "",
    audioDeviceId: "",
    ttsModel: "turbo",
  };
}

function loadConfig(): BroadcastConfig {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (raw) return { ...defaultConfig(), ...JSON.parse(raw) };
  } catch {}
  return defaultConfig();
}

function saveConfig(cfg: BroadcastConfig) {
  localStorage.setItem(STORAGE_KEY, JSON.stringify(cfg));
}

export function useBroadcastConfig() {
  const [config, setConfig] = useState(loadConfig);

  useEffect(() => { saveConfig(config); }, [config]);

  const update = <K extends keyof BroadcastConfig>(key: K, value: BroadcastConfig[K]) =>
    setConfig((prev) => ({ ...prev, [key]: value }));

  return { config, update, setConfig } as const;
}
