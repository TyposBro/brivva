import { useCallback, useMemo, useState } from "react";
import { cn } from "../../../core/cn";
import {
  detectWebCodecsSupport,
  mediaIngestModeLabel,
  readStoredMediaIngestMode,
  resolveMediaIngestMode,
  webCodecsFrontendEnabled,
  writeStoredMediaIngestMode,
  type MediaIngestMode,
} from "./media-ingest-mode";

export function useMediaIngestModePreference() {
  const [mode, setModeState] = useState<MediaIngestMode>(() => readStoredMediaIngestMode());
  const setMode = useCallback((next: MediaIngestMode) => {
    setModeState(next);
    writeStoredMediaIngestMode(next);
  }, []);
  return [mode, setMode] as const;
}

export function AdvancedMediaIngestSettings({
  value,
  onChange,
}: {
  value: MediaIngestMode;
  onChange: (mode: MediaIngestMode) => void;
}) {
  const support = useMemo(() => detectWebCodecsSupport(), []);
  const frontendEnabled = webCodecsFrontendEnabled();
  const webCodecsDisabled = !frontendEnabled || !support.available;
  const webCodecsUnavailableCopy = !frontendEnabled
    ? "Frontend flag VITE_WEBCODECS_INGEST_ENABLED is off."
    : `Missing ${support.reasons.join(", ")}.`;

  return (
    <details className="bg-surface-container-low rounded-xl border border-surface-container-high p-4 group">
      <summary className="cursor-pointer list-none font-headline font-bold text-on-surface flex items-center justify-between gap-3">
        <span>Advanced media settings</span>
        <span className="text-[10px] font-label uppercase tracking-widest text-on-surface-variant group-open:hidden">
          {mediaIngestModeLabel(value)}
        </span>
      </summary>

      <div className="mt-4 space-y-3">
        <div>
          <p className="font-label font-bold text-on-surface text-sm">Media ingest mode</p>
          <p className="font-label text-xs text-on-surface-variant mt-1">
            Pick the camera ingest path for this browser/session before pressing Record.
          </p>
        </div>

        <ModeRadio
          mode="auto"
          checked={value === "auto"}
          onChange={onChange}
          title="Auto (recommended)"
          description="Use Brivva's safest current production path for this browser."
          badges={["Available", resolveMediaIngestMode("auto") === "webrtc" ? "Using fallback: WebRTC" : ""]}
        />
        <ModeRadio
          mode="webrtc"
          checked={value === "webrtc"}
          onChange={onChange}
          title="WebRTC hardened"
          description="Browser sends camera video through WebRTC. Lower bandwidth, but depends on ICE/TURN/browser codec behavior."
          badges={["Available"]}
        />
        <ModeRadio
          mode="webcodecs_ws"
          checked={value === "webcodecs_ws"}
          disabled={webCodecsDisabled}
          onChange={onChange}
          title="WebCodecs over WebSocket (experimental)"
          description="Browser encodes video frames directly and sends them through the same secure media socket as audio. More deterministic and easier to debug, but newer."
          badges={[
            "Experimental",
            webCodecsDisabled ? "Unavailable in this browser" : "Available",
          ]}
        />

        {value === "webcodecs_ws" && webCodecsDisabled && (
          <p className="text-error text-xs font-label bg-error-container/20 rounded-lg px-3 py-2">
            WebCodecs ingest is not available in this browser. Use WebRTC or switch to Chrome/Brave. {webCodecsUnavailableCopy}
          </p>
        )}
      </div>
    </details>
  );
}

function ModeRadio({
  mode,
  checked,
  disabled = false,
  onChange,
  title,
  description,
  badges,
}: {
  mode: MediaIngestMode;
  checked: boolean;
  disabled?: boolean;
  onChange: (mode: MediaIngestMode) => void;
  title: string;
  description: string;
  badges: string[];
}) {
  return (
    <label
      className={cn(
        "block rounded-lg border p-3 transition-colors",
        checked
          ? "border-primary bg-primary/5"
          : "border-surface-container-highest bg-surface-container",
        disabled ? "opacity-60 cursor-not-allowed" : "cursor-pointer hover:border-primary/60",
      )}
    >
      <div className="flex gap-3">
        <input
          type="radio"
          name="media-ingest-mode"
          value={mode}
          checked={checked}
          disabled={disabled}
          onChange={() => onChange(mode)}
          className="mt-1 accent-primary"
        />
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-2">
            <span className="font-label font-bold text-on-surface text-sm">{title}</span>
            {badges.filter(Boolean).map((badge) => (
              <span
                key={badge}
                className={cn(
                  "text-[10px] font-label font-bold uppercase tracking-widest px-1.5 py-0.5 rounded",
                  badge.includes("Unavailable")
                    ? "bg-error-container/30 text-error"
                    : badge.includes("Experimental")
                      ? "bg-warning/10 text-warning"
                      : "bg-primary/10 text-primary",
                )}
              >
                {badge}
              </span>
            ))}
          </div>
          <p className="font-label text-xs text-on-surface-variant mt-1 leading-relaxed">
            {description}
          </p>
        </div>
      </div>
    </label>
  );
}
