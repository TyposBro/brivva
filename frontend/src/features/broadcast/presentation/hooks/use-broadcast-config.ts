import { useState, useEffect } from "react";
import type { TranslationTier, VoiceMode } from "../../domain/broadcast-types";
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
  voiceConfig: Record<string, VoiceMode>;
};

const DEFAULT_SOURCE_LANG = "en";
const DEFAULT_TARGET_LANGS = ["ja", "ko"];
const DEFAULT_TIER: TranslationTier = 2;
const DEFAULT_BROADCAST_DELAY_MS = 5000;
const DEFAULT_TTS_MODEL: BroadcastConfig["ttsModel"] = "turbo";
const DEFAULT_TTS_PROVIDER: TtsProvider = "elevenlabs";
const DEFAULT_VOICE_CONFIG: Record<string, VoiceMode> = { zh: "default-female" };

const VALID_VOICE_MODES: VoiceMode[] = ["cloned", "default-female", "default-male"];

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
    voiceConfig: DEFAULT_VOICE_CONFIG,
  };
}

const VALID_TTS_MODELS: BroadcastConfig["ttsModel"][] = ["turbo", "flash"];
const VALID_TTS_PROVIDERS: TtsProvider[] = ["elevenlabs", "dashscope"];

/** Migrate old config format (voiceDefaultLangs + ttsVoiceGender) to voiceConfig. */
function migrateConfig(cfg: Record<string, unknown>): void {
  if (cfg.voiceConfig) {
    const vc = cfg.voiceConfig as Record<string, string>;
    for (const [lang, mode] of Object.entries(vc)) {
      if (!VALID_VOICE_MODES.includes(mode as VoiceMode)) {
        vc[lang] = "cloned";
      }
    }
    return;
  }
  const gender = cfg.ttsVoiceGender === "male" ? "male" : "female";
  const defaultLangs = Array.isArray(cfg.voiceDefaultLangs) ? cfg.voiceDefaultLangs as string[] : [];
  const voiceConfig: Record<string, VoiceMode> = {};
  for (const lang of defaultLangs) {
    voiceConfig[lang] = `default-${gender}` as VoiceMode;
  }
  cfg.voiceConfig = voiceConfig;
  delete cfg.voiceDefaultLangs;
  delete cfg.ttsVoiceGender;
}

function loadConfig(): BroadcastConfig {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (raw) {
      const parsed = JSON.parse(raw);
      migrateConfig(parsed);
      const cfg = { ...defaultConfig(), ...parsed };
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

/** Derive voiceDefaultLangs from voiceConfig (for WebSocket backward compat). */
export function deriveVoiceDefaultLangs(vc: Record<string, VoiceMode>): string[] {
  return Object.entries(vc)
    .filter(([, mode]) => mode !== "cloned")
    .map(([lang]) => lang);
}

/** Derive per-language gender map from voiceConfig: "ja:female,zh:male". */
export function deriveVoiceGenderMap(vc: Record<string, VoiceMode>): string {
  return Object.entries(vc)
    .filter(([, mode]) => mode !== "cloned")
    .map(([lang, mode]) => `${lang}:${mode.replace("default-", "")}`)
    .join(",");
}

export function useBroadcastConfig() {
  const [config, setConfig] = useState(loadConfig);

  useEffect(() => { saveConfig(config); }, [config]);

  const update = <K extends keyof BroadcastConfig>(key: K, value: BroadcastConfig[K]) =>
    setConfig((prev) => ({ ...prev, [key]: value }));

  return { config, update, setConfig } as const;
}
