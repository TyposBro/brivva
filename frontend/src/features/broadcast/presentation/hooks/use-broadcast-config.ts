import { useState, useEffect } from "react";
import type { TranslationTier } from "../../domain/broadcast-types";
import { STORAGE_KEY } from "../../domain/broadcast-constants";

export type TtsProvider = "elevenlabs" | "dashscope";

export type BroadcastConfig = {
  sourceLang: string;
  targetLangs: string[];
  tier: TranslationTier;
  rtmpUrls: Record<string, string>;
  broadcastDelay: number;
  videoDeviceId: string;
  audioDeviceId: string;
  ttsModel: "turbo" | "flash";
  ttsProvider: TtsProvider;
};

const DEFAULT_SOURCE_LANG = "en";
const DEFAULT_TARGET_LANGS = ["ja", "ko"];
const DEFAULT_TIER: TranslationTier = 2;
const DEFAULT_BROADCAST_DELAY_MS = 5000;
const DEFAULT_TTS_MODEL: BroadcastConfig["ttsModel"] = "turbo";
const DEFAULT_TTS_PROVIDER: TtsProvider = "elevenlabs";

function defaultConfig(): BroadcastConfig {
  return {
    sourceLang: DEFAULT_SOURCE_LANG,
    targetLangs: DEFAULT_TARGET_LANGS,
    tier: DEFAULT_TIER,
    rtmpUrls: {},
    broadcastDelay: DEFAULT_BROADCAST_DELAY_MS,
    videoDeviceId: "",
    audioDeviceId: "",
    ttsModel: DEFAULT_TTS_MODEL,
    ttsProvider: DEFAULT_TTS_PROVIDER,
  };
}

const VALID_TTS_MODELS: BroadcastConfig["ttsModel"][] = ["turbo", "flash"];
const VALID_TTS_PROVIDERS: TtsProvider[] = ["elevenlabs", "dashscope"];

function loadConfig(): BroadcastConfig {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (raw) {
      const cfg = { ...defaultConfig(), ...JSON.parse(raw) };
      if (!VALID_TTS_MODELS.includes(cfg.ttsModel)) cfg.ttsModel = DEFAULT_TTS_MODEL;
      if (!VALID_TTS_PROVIDERS.includes(cfg.ttsProvider)) cfg.ttsProvider = DEFAULT_TTS_PROVIDER;
      return cfg;
    }
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
