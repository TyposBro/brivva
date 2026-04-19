// Local cache of the user's preferred default target language. Workers will
// own this server-side as part of Round 2; until then we mirror the choice in
// localStorage so the dashboard can pre-populate destinations on next launch.

const KEY = "brivva_default_target_lang";

export function readDefaultTargetLang(): string | null {
  try {
    return localStorage.getItem(KEY);
  } catch {
    return null;
  }
}

export function writeDefaultTargetLang(lang: string): void {
  try {
    localStorage.setItem(KEY, lang);
  } catch {
    // SSR / private mode — preference will be re-asked next session.
  }
}
