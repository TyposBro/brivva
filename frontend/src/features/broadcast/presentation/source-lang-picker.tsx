import { Check } from "lucide-react";
import { cn } from "../../../core/cn";
import * as broadcastApi from "../data/api-client";

export type SourceLang = "ko" | "en" | "ja" | "zh";
export const SOURCE_LANGS: readonly SourceLang[] = ["ko", "en", "ja", "zh"];

/** Pick a source language from `navigator.language`, falling back to Korean when
 *  the browser locale isn't one of the four voice-clone languages. Shared by
 *  onboarding + dashboard so both flows stay in sync. */
export function detectBrowserSourceLang(): SourceLang {
  const tag = (typeof navigator !== "undefined" && navigator.language) || "";
  const prefix = tag.toLowerCase().split("-")[0];
  return (SOURCE_LANGS as readonly string[]).includes(prefix)
    ? (prefix as SourceLang)
    : "ko";
}

interface SourceLangPickerProps {
  value: SourceLang;
  onChange: (lang: SourceLang) => void;
  disabled?: boolean;
  /** Shows the browser-default hint. Only set when the current value came from
   *  `detectBrowserSourceLang()`, not a persisted voice. */
  showBrowserDefaultHint?: boolean;
  /** Prompt above the radio group. Differs slightly between onboarding and
   *  the dashboard re-record flow. */
  label?: string;
  id?: string;
}

export function SourceLangPicker({
  value,
  onChange,
  disabled = false,
  showBrowserDefaultHint = false,
  label = "What language will you speak on stream?",
  id = "source-lang",
}: SourceLangPickerProps) {
  return (
    <div>
      <label
        htmlFor={id}
        className="block font-label text-sm text-on-surface mb-2"
      >
        {label}
      </label>
      <div id={id} role="radiogroup" className="grid grid-cols-2 gap-2">
        {SOURCE_LANGS.map((code) => {
          const meta = broadcastApi.LANGS.find((l) => l.code === code)!;
          const selected = value === code;
          return (
            <button
              key={code}
              type="button"
              role="radio"
              aria-checked={selected}
              disabled={disabled}
              className={cn(
                "flex items-center gap-2 px-4 py-2.5 rounded-lg font-label text-sm transition-colors text-left disabled:opacity-50",
                selected
                  ? "bg-primary-container text-on-primary-container"
                  : "bg-surface-container-low hover:bg-surface-container-high text-on-surface",
              )}
              onClick={() => onChange(code)}
            >
              <span className="text-lg">{meta.flag}</span>
              <span className="flex-1">{meta.label}</span>
              {selected && <Check className="w-4 h-4" />}
            </button>
          );
        })}
      </div>
      {showBrowserDefaultHint && (
        <p className="text-on-surface-variant text-xs font-label mt-2">
          Defaulted from your browser. Change this if you'll be recording in a
          different language.
        </p>
      )}
    </div>
  );
}
