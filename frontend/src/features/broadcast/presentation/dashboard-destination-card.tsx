import { useState } from "react";
import {
  ChevronDown,
  ChevronRight,
  ExternalLink,
  Globe,
  Monitor,
  Sliders,
  Tv,
  X,
  Youtube,
} from "lucide-react";
import { cn } from "../../../core/cn";
import * as api from "../data/api-client";
import { isStreamDefault } from "../../../core/config/stream-defaults";

export type Destination = {
  uid: string;
  platform: string;
  lang: string;
  rtmp_url: string;
  stream_key: string;
  /** Fargate holds the original host media this long (ms) before pushing to
   *  RTMP. Default 2000 ms covers typical STT + TTS latency. */
  delay_ms: number;
  /** 0.0–1.0: how loud the delayed original audio sits under translated TTS.
   *  1.0 = pure passthrough (source lang), 0.2 = ducked underlay (target). */
  host_gain: number;
};

// Platforms whose only "auth" is pasting an RTMP URL + stream key from the
// host dashboard. We surface a Save button in the config panel so the values
// get persisted into platform_credentials and pre-filled on the next session
// without pushing the user through a separate "Connected platforms" page.
const PASTE_CREDS_PLATFORMS = new Set(["grip", "tiktok"]);

export function PlatformIcon({ id, className }: { id: string; className?: string }) {
  switch (id) {
    case "youtube":
      return <Youtube className={className} />;
    case "twitch":
      return <Tv className={className} />;
    case "local-test":
      return <Monitor className={className} />;
    default:
      return <Globe className={className} />;
  }
}

function StreamSlider({
  label,
  valueLabel,
  min,
  max,
  step,
  value,
  onChange,
  hint,
}: {
  label: string;
  valueLabel: string;
  min: number;
  max: number;
  step: number;
  value: number;
  onChange: (v: number) => void;
  hint?: string;
}) {
  return (
    <div>
      <div className="flex items-baseline justify-between gap-3 mb-1">
        <span className="text-[11px] font-label text-on-surface-variant uppercase tracking-wider">
          {label}
        </span>
        <span className="text-xs font-label font-bold text-on-surface tabular-nums">
          {valueLabel}
        </span>
      </div>
      <input
        type="range"
        min={min}
        max={max}
        step={step}
        value={value}
        onChange={(e) => onChange(Number(e.target.value))}
        className="w-full accent-primary h-1"
      />
      {hint && (
        <p className="text-[10px] text-on-surface-variant/60 leading-snug mt-1">
          {hint}
        </p>
      )}
    </div>
  );
}

type PlatformRow = (typeof api.PLATFORMS)[number];

function LangSelect({ dest, sourceLang, onUpdate }: { dest: Destination; sourceLang: string; onUpdate: (patch: Partial<Destination>) => void }) {
  const isPass = api.isPassthroughLang(dest.lang);
  return (
    <div className="relative flex items-center gap-1.5">
      {isPass && (
        <span
          className="px-1.5 py-0.5 rounded bg-surface-container-highest text-[10px] font-label font-bold uppercase tracking-wider text-primary"
          title="No STT / translate / TTS — host audio goes through unchanged"
        >
          raw
        </span>
      )}
      <select
        className={cn(
          "appearance-none bg-surface-container-highest border-none rounded px-2.5 py-1 text-on-surface font-label text-xs focus:ring-2 focus:ring-primary/50 outline-none pr-6 cursor-pointer",
          isPass && "italic",
        )}
        value={dest.lang}
        onChange={(e) => onUpdate({ lang: e.target.value })}
      >
        {api.LANGS.filter((l) => l.code !== sourceLang).map((l) => (
          <option key={l.code} value={l.code}>
            {l.flag} {l.label}
          </option>
        ))}
      </select>
      <ChevronDown className="absolute right-1.5 top-1/2 -translate-y-1/2 w-3 h-3 text-on-surface-variant pointer-events-none" />
    </div>
  );
}

function CardHeader(props: {
  dest: Destination;
  platform: PlatformRow;
  sourceLang: string;
  savedCreds: Record<string, api.PlatformCredential>;
  expanded: boolean;
  setExpanded: (v: boolean) => void;
  onUpdate: (patch: Partial<Destination>) => void;
  onRemove: () => void;
}) {
  const { dest, platform, sourceLang, savedCreds, expanded, setExpanded, onUpdate, onRemove } = props;
  const needsConfig = !platform.auto || PASTE_CREDS_PLATFORMS.has(dest.platform);
  const hasConfig = !!(dest.rtmp_url || dest.stream_key);
  const hasSavedCreds = !!savedCreds[dest.platform];

  return (
    <div className="flex items-center gap-3 px-4 py-3">
      <PlatformIcon id={dest.platform} className="w-4 h-4 text-primary" />
      <span className="font-label font-bold text-on-surface text-sm flex-1 truncate">{platform.label}</span>
      <LangSelect dest={dest} sourceLang={sourceLang} onUpdate={onUpdate} />
      {needsConfig && (
        <span
          className={cn("w-2 h-2 rounded-full shrink-0", hasConfig || hasSavedCreds ? "bg-success" : "bg-error/60")}
          title={hasConfig || hasSavedCreds ? "Configured" : "Needs stream key"}
        />
      )}
      {needsConfig && (
        <button className="text-on-surface-variant hover:text-on-surface p-1 transition-colors" onClick={() => setExpanded(!expanded)}>
          {expanded ? <ChevronDown className="w-3.5 h-3.5" /> : <ChevronRight className="w-3.5 h-3.5" />}
        </button>
      )}
      <button className="text-on-surface-variant hover:text-error transition-colors p-1" onClick={onRemove}>
        <X className="w-3.5 h-3.5" />
      </button>
    </div>
  );
}

function TimingSliders({ dest, sourceLang, onUpdate }: { dest: Destination; sourceLang: string; onUpdate: (patch: Partial<Destination>) => void }) {
  const isSource = dest.lang === sourceLang;
  const isPass = api.isPassthroughLang(dest.lang);
  // Passthrough destinations skip STT/translate/TTS entirely — the under-voice
  // mixer has nothing to mix. Sliders would mislead the user into thinking
  // they can tune something the pipeline won't honor.
  if (isPass) {
    return (
      <div className="px-4 pb-3 pt-1">
        <p className="text-[11px] text-on-surface-variant/70 leading-snug">
          Passthrough destinations re-broadcast the host's raw audio and video
          at full volume. Translation pipeline is skipped, so delay and under-
          voice mix do not apply.
        </p>
      </div>
    );
  }
  return (
    <div className="px-4 pb-3 pt-1 space-y-2">
      <StreamSlider
        label="Output delay"
        valueLabel={`${dest.delay_ms} ms`}
        min={0} max={5000} step={100}
        value={dest.delay_ms}
        onChange={(v) => onUpdate({ delay_ms: v })}
        hint="Fargate holds the original media this long before emitting, leaving time for STT + translate + TTS."
      />
      <StreamSlider
        label={isSource ? "Original audio volume" : "Under-voice volume"}
        valueLabel={`${Math.round(dest.host_gain * 100)}%`}
        min={0} max={100} step={5}
        value={Math.round(dest.host_gain * 100)}
        onChange={(v) => onUpdate({ host_gain: v / 100 })}
        hint={isSource
          ? "100% for source streams — no translation overlay to duck under."
          : "How loud the original voice sits under the translated speech."}
      />
    </div>
  );
}

function ConfigPanel(props: {
  dest: Destination;
  platform: PlatformRow;
  hasSavedCreds: boolean;
  userId: string;
  onUpdate: (patch: Partial<Destination>) => void;
  onCredentialSaved?: (cred: api.PlatformCredential) => void;
}) {
  const { dest, platform, hasSavedCreds, userId, onUpdate, onCredentialSaved } = props;
  const supportsSave = PASTE_CREDS_PLATFORMS.has(dest.platform);
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [savedJustNow, setSavedJustNow] = useState(false);

  const handleSave = async () => {
    if (!supportsSave) return;
    if (!dest.stream_key) {
      setSaveError("Stream key is required");
      return;
    }
    setSaving(true);
    setSaveError(null);
    try {
      const body = {
        user_id: userId,
        // The workers endpoint requires a session_token. For the MVP paste
        // flow we don't actually have a meaningful token — pass the stream
        // key's tail as a stable fingerprint so the display_name default
        // works and the backend validation passes.
        session_token: dest.stream_key.slice(-12) || dest.stream_key,
        stream_key: dest.stream_key,
        rtmp_url: dest.rtmp_url || undefined,
        display_name: `${platform.label} (pasted)`,
      };
      const cred =
        dest.platform === "grip"
          ? await api.saveGripAuth(body)
          : await api.saveTikTokAuth(body);
      setSavedJustNow(true);
      onCredentialSaved?.(cred);
    } catch (e) {
      setSaveError(e instanceof Error ? e.message : "Failed to save");
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="px-4 pb-4 space-y-2">
      {hasSavedCreds && !savedJustNow && (
        <span className="text-[10px] font-label text-success uppercase tracking-widest">
          Pre-filled from saved credentials
        </span>
      )}
      {savedJustNow && (
        <span className="text-[10px] font-label text-success uppercase tracking-widest">
          Saved — will pre-fill next session
        </span>
      )}
      {platform.settingsUrl && (
        <a href={platform.settingsUrl} target="_blank" rel="noopener noreferrer"
          className="flex items-center gap-1 text-primary text-xs font-label hover:underline">
          Open {platform.label} Settings <ExternalLink className="w-3 h-3" />
        </a>
      )}
      <p className="text-on-surface-variant/60 text-xs leading-relaxed">{platform.help}</p>
      <div className="grid grid-cols-1 sm:grid-cols-2 gap-2">
        {!platform.keyOnly && (
          <input
            className="bg-surface-container-highest border-none rounded-lg px-3 py-2 text-on-surface placeholder:text-on-surface-variant/50 focus:ring-2 focus:ring-primary/50 transition-all font-label text-sm outline-none"
            placeholder="Server URL" value={dest.rtmp_url}
            onChange={(e) => onUpdate({ rtmp_url: e.target.value })}
          />
        )}
        <input
          className="bg-surface-container-highest border-none rounded-lg px-3 py-2 text-on-surface placeholder:text-on-surface-variant/50 focus:ring-2 focus:ring-primary/50 transition-all font-label text-sm outline-none"
          placeholder="Stream Key" type="password" value={dest.stream_key}
          onChange={(e) => onUpdate({ stream_key: e.target.value })}
        />
      </div>
      {supportsSave && (
        <div className="flex items-center gap-2 pt-1">
          <button
            type="button"
            className="bg-primary hover:bg-primary/90 disabled:bg-primary/40 text-on-primary px-3 py-1.5 rounded-lg text-xs font-label font-bold transition-colors"
            onClick={handleSave}
            disabled={saving || !dest.stream_key}
          >
            {saving ? "Saving..." : "Save credentials"}
          </button>
          {saveError && (
            <span className="text-error text-xs font-label" role="alert">
              {saveError}
            </span>
          )}
        </div>
      )}
    </div>
  );
}

export function DestinationCard(props: {
  dest: Destination;
  sourceLang: string;
  savedCreds: Record<string, api.PlatformCredential>;
  user: api.UserInfo | null;
  userId: string;
  privacyStatus: string;
  onPrivacyChange: (v: string) => void;
  onUpdate: (patch: Partial<Destination>) => void;
  onRemove: () => void;
  /** Called after the user saves pasted creds for a paste-only platform
   *  (Grip / TikTok). The parent syncs the result into `savedCreds` so the
   *  next card/session finds the pre-filled values. */
  onCredentialSaved?: (cred: api.PlatformCredential) => void;
  /** Per-destination client-side validation error. When set, the card shows a
   *  red inline message and the dashboard disables Go Live. `null` = valid. */
  validationError?: string | null;
}) {
  const { dest, sourceLang, savedCreds, userId, onUpdate, onRemove, onCredentialSaved, validationError } = props;
  const [expanded, setExpanded] = useState<boolean>(() => {
    const p = api.PLATFORMS.find((x) => x.id === dest.platform);
    if (!p) return false;
    // For paste-creds platforms (grip/tiktok) we pretend the card "needs
    // config" when we don't already have saved creds, even though
    // `platform.auto` may be true (Grip has a Seller API path for later).
    const pasteOnly = PASTE_CREDS_PLATFORMS.has(dest.platform);
    const needsConfig = !p.auto || pasteOnly;
    return needsConfig && !savedCreds[dest.platform];
  });
  // Pre-expand timing sliders only when the user has already overridden a
  // default — otherwise the curated stream-default is correct and we hide it
  // behind an "Advanced" toggle to keep the card compact.
  const [advancedOpen, setAdvancedOpen] = useState(() =>
    !isStreamDefault(sourceLang, dest.lang, {
      delay_ms: dest.delay_ms,
      host_gain: dest.host_gain,
    }),
  );
  const platform = api.PLATFORMS.find((p) => p.id === dest.platform);
  if (!platform) return null;
  const pasteOnly = PASTE_CREDS_PLATFORMS.has(dest.platform);
  const needsConfig = !platform.auto || pasteOnly;
  const hasSavedCreds = !!savedCreds[dest.platform];

  return (
    <div
      className={cn(
        "bg-surface-container-low rounded-xl overflow-hidden",
        validationError && "ring-1 ring-error/60",
      )}
    >
      <CardHeader
        dest={dest} platform={platform} sourceLang={sourceLang}
        savedCreds={savedCreds} expanded={expanded} setExpanded={setExpanded}
        onUpdate={onUpdate} onRemove={onRemove}
      />

      {validationError && (
        <p
          className="px-4 pb-2 -mt-1 text-error text-xs font-label"
          role="alert"
        >
          {validationError}
        </p>
      )}

      <button
        className="w-full flex items-center gap-2 px-4 py-2 text-on-surface-variant hover:text-on-surface hover:bg-surface-container-high transition-colors text-[11px] font-label uppercase tracking-widest"
        onClick={() => setAdvancedOpen((v) => !v)}
      >
        <Sliders className="w-3 h-3" />
        Advanced
        {advancedOpen ? (
          <ChevronDown className="w-3 h-3 ml-auto" />
        ) : (
          <ChevronRight className="w-3 h-3 ml-auto" />
        )}
      </button>

      {advancedOpen && (
        <TimingSliders dest={dest} sourceLang={sourceLang} onUpdate={onUpdate} />
      )}
      {needsConfig && expanded && (
        <ConfigPanel
          dest={dest}
          platform={platform}
          hasSavedCreds={hasSavedCreds}
          userId={userId}
          onUpdate={onUpdate}
          onCredentialSaved={onCredentialSaved}
        />
      )}
    </div>
  );
}
