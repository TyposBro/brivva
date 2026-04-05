import { LANGS, PLATFORMS } from "./data";

export function langLabel(code: string): string {
  return LANGS.find((l) => l.code === code)?.label ?? code;
}

export function langFlag(code: string): string {
  return LANGS.find((l) => l.code === code)?.flag ?? "";
}

export function getPlatformLabel(platformId: string): string {
  return PLATFORMS.find((p) => p.id === platformId)?.label ?? platformId;
}
