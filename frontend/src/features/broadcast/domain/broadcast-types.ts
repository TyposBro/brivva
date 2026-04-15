export type Lang = { code: string; label: string; flag: string };

export const LANGS: Lang[] = [
  { code: "ko", label: "Korean", flag: "\uD83C\uDDF0\uD83C\uDDF7" },
  { code: "en", label: "English", flag: "\uD83C\uDDEC\uD83C\uDDE7" },
  { code: "ja", label: "Japanese", flag: "\uD83C\uDDEF\uD83C\uDDF5" },
  { code: "zh", label: "Chinese", flag: "\uD83C\uDDE8\uD83C\uDDF3" },
  { code: "ru", label: "Russian", flag: "\uD83C\uDDF7\uD83C\uDDFA" },
] as const;

export type VoiceMode = "cloned" | "default-female" | "default-male";

export type TranslationTier = 1 | 2 | 3 | 4;

export type TranscriptEntry = {
  id: number;
  text: string;
  translations: Record<string, string>;
};
