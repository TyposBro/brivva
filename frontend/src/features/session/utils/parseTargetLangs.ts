export function parseTargetLangs(raw: string): string[] {
  try {
    return JSON.parse(raw);
  } catch {
    return [];
  }
}
