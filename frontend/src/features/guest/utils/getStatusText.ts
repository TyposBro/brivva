import { type Lang, LANG_LABELS } from "../../../types";

const STATUS_LABELS: Record<string, string | ((lang: Lang) => string)> = {
  connecting: "Connecting\u2026",
  listening: (lang: Lang) => `Listening \u00b7 ${LANG_LABELS[lang]}`,
  closed: "Host disconnected",
  error: "Connection error",
};

export function getStatusText(status: string, lang: Lang): string {
  const entry = STATUS_LABELS[status];
  if (!entry) return "";
  return typeof entry === "function" ? entry(lang) : entry;
}
