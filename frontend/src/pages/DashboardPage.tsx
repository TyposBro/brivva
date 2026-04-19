import { useEffect, useState, useCallback, useRef } from "react";
import { useNavigate, useSearchParams } from "react-router-dom";
import {
  Mic,
  Trash2,
  Plus,
  X,
  Radio,
  ExternalLink,
  ChevronDown,
  ChevronRight,
  Youtube,
  Tv,
  Globe,
  Monitor,
  Settings2,
  Clipboard,
} from "lucide-react";
import { cn } from "../core/cn";
import * as api from "../features/broadcast/data/api-client";

// ── Types ──────────────────────────────────────────────

type Destination = {
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

const DEFAULT_DELAY_MS = 2000;
const DEFAULT_HOST_GAIN_TARGET = 0.2;
const DEFAULT_HOST_GAIN_SOURCE = 1.0;

function getUserId(): string {
  let id = localStorage.getItem("brivva_user_id");
  if (!id) {
    id = crypto.randomUUID();
    localStorage.setItem("brivva_user_id", id);
  }
  return id;
}

// ── Platform icons ─────────────────────────────────────

function PlatformIcon({ id, className }: { id: string; className?: string }) {
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

// ── Region grouping for the picker ─────────────────────

const REGION_GROUPS = [
  { key: "Global", label: "Global", icon: "🌐" },
  { key: "Korea", label: "Korean Platforms", icon: "🇰🇷" },
  { key: "Japan", label: "Japanese Platforms", icon: "🇯🇵" },
  { key: "China", label: "Chinese Platforms", icon: "🇨🇳" },
  { key: "Other", label: "Other", icon: "⚙️" },
];

// ── Component ──────────────────────────────────────────

export default function DashboardPage() {
  const navigate = useNavigate();
  const [searchParams] = useSearchParams();
  const userId = getUserId();

  const [user, setUser] = useState<api.UserInfo | null>(null);
  const [voices, setVoices] = useState<api.Voice[]>([]);
  const [sessions, setSessions] = useState<api.Session[]>([]);
  const [loading, setLoading] = useState(true);
  const [savedCreds, setSavedCreds] = useState<
    Record<string, api.PlatformCredential>
  >({});

  // Session form
  const [title, setTitle] = useState("");
  const [sourceLang, setSourceLang] = useState("ko");
  const [selectedVoice, setSelectedVoice] = useState("");
  const [destinations, setDestinations] = useState<Destination[]>([]);
  const [creating, setCreating] = useState(false);
  const [error, setError] = useState("");
  const [privacyStatus, setPrivacyStatus] = useState("unlisted");
  const [magicPaste, setMagicPaste] = useState("");

  // Progressive disclosure
  const [pickerOpen, setPickerOpen] = useState(false);
  const [expandedRegion, setExpandedRegion] = useState<string | null>(null);
  const [showSettings, setShowSettings] = useState(false);
  const pickerRef = useRef<HTMLDivElement>(null);

  const loadData = useCallback(async () => {
    try {
      const [u, v, s, c] = await Promise.all([
        api.getUser(userId),
        api.listVoices(userId),
        api.listSessions(userId),
        api.listCredentials(userId),
      ]);
      setUser(u);
      setVoices(v.voices);
      setSessions(s.sessions);
      const credMap: Record<string, api.PlatformCredential> = {};
      for (const cred of c.credentials) credMap[cred.platform] = cred;
      setSavedCreds(credMap);
    } catch (e) {
      console.error("Failed to load data:", e);
    } finally {
      setLoading(false);
    }
  }, [userId]);

  useEffect(() => {
    loadData();
  }, [loadData]);

  // Close picker on outside click
  useEffect(() => {
    const handler = (e: MouseEvent) => {
      if (pickerRef.current && !pickerRef.current.contains(e.target as Node)) {
        setPickerOpen(false);
      }
    };
    if (pickerOpen) document.addEventListener("mousedown", handler);
    return () => document.removeEventListener("mousedown", handler);
  }, [pickerOpen]);

  const youtubeJustConnected = searchParams.get("youtube") === "connected";

  // ── Destination management ─────────────────────────

  const addDestination = (platformId: string) => {
    const platform = api.PLATFORMS.find((p) => p.id === platformId);
    if (!platform) return;

    const autoLang = api.PLATFORM_LANG[platformId];
    const lang = autoLang ?? (sourceLang === "en" ? "ja" : "en");
    const cred = savedCreds[platformId];

    setDestinations((prev) => [
      ...prev,
      {
        uid: crypto.randomUUID(),
        platform: platformId,
        lang,
        rtmp_url: cred?.rtmp_url ?? platform.defaultRtmp ?? "",
        stream_key: cred?.stream_key ?? "",
        delay_ms: DEFAULT_DELAY_MS,
        host_gain:
          lang === sourceLang
            ? DEFAULT_HOST_GAIN_SOURCE
            : DEFAULT_HOST_GAIN_TARGET,
      },
    ]);
    setPickerOpen(false);
    setExpandedRegion(null);
  };

  const removeDestination = (uid: string) => {
    setDestinations((prev) => prev.filter((d) => d.uid !== uid));
  };

  const updateDestination = (uid: string, patch: Partial<Destination>) => {
    setDestinations((prev) =>
      prev.map((d) => (d.uid === uid ? { ...d, ...patch } : d))
    );
  };

  const handleMagicPaste = (value: string) => {
    setMagicPaste(value);
    const detected = api.detectPlatform(value);
    if (detected) {
      const autoLang = api.PLATFORM_LANG[detected.platform];
      const destLang = autoLang ?? "en";
      setDestinations((prev) => [
        ...prev,
        {
          uid: crypto.randomUUID(),
          platform: detected.platform,
          lang: destLang,
          rtmp_url: detected.rtmpUrl,
          stream_key: detected.streamKey,
          delay_ms: DEFAULT_DELAY_MS,
          host_gain:
            destLang === sourceLang
              ? DEFAULT_HOST_GAIN_SOURCE
              : DEFAULT_HOST_GAIN_TARGET,
        },
      ]);
      setMagicPaste("");
    }
  };

  // ── Create session ─────────────────────────────────

  const handleCreateSession = async () => {
    if (!title.trim()) {
      setError("Enter a session title");
      return;
    }
    if (destinations.length === 0) {
      setError("Add at least one destination");
      return;
    }

    for (const dest of destinations) {
      if (dest.lang === sourceLang) {
        setError(
          `${api.langLabel(dest.lang)} is your source language — remove or change the destination`
        );
        return;
      }
      const p = api.PLATFORMS.find((x) => x.id === dest.platform);
      if (dest.platform === "youtube" && !user?.youtube_connected) {
        setError("Connect your YouTube account first");
        return;
      }
      if (p && !p.auto) {
        if (!p.keyOnly && !dest.rtmp_url) {
          setError(`Enter server URL for ${p.label}`);
          return;
        }
        if (!dest.stream_key) {
          setError(`Enter stream key for ${p.label}`);
          return;
        }
      }
    }

    setCreating(true);
    setError("");

    try {
      const targetLangs = [...new Set(destinations.map((d) => d.lang))];
      const platforms: api.PlatformConfig[] = destinations.map((d) => {
        const p = api.PLATFORMS.find((x) => x.id === d.platform);
        const base = {
          platform: d.platform,
          lang: d.lang,
          delay_ms: d.delay_ms,
          host_gain: d.host_gain,
        };
        if (p?.auto) return base;
        const rtmpUrl = p?.keyOnly
          ? p.defaultRtmp
          : d.rtmp_url || p?.defaultRtmp || "";
        return {
          ...base,
          rtmp_url: rtmpUrl,
          stream_key: d.stream_key,
        };
      });

      const hasYoutube = destinations.some((d) => d.platform === "youtube");

      const result = await api.createSession({
        user_id: userId,
        title: title.trim(),
        source_lang: sourceLang,
        target_langs: targetLangs,
        voice_id: selectedVoice || undefined,
        platforms,
        privacy_status: hasYoutube ? privacyStatus : undefined,
      });

      if (result.errors?.length) setError(result.errors.join("; "));
      navigate(`/session/${result.session.id}`);
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : "Failed to create session");
      setCreating(false);
    }
  };

  const handleDeleteVoice = async (voiceId: string) => {
    await api.deleteVoice(voiceId);
    setVoices((prev) => prev.filter((v) => v.id !== voiceId));
    if (selectedVoice === voiceId) setSelectedVoice("");
  };

  // ── Render ─────────────────────────────────────────

  if (loading) {
    return (
      <div className="min-h-screen bg-background flex items-center justify-center">
        <p className="text-on-surface-variant font-label">Loading...</p>
      </div>
    );
  }

  const hasYoutubeDest = destinations.some((d) => d.platform === "youtube");

  return (
    <div className="min-h-screen bg-background">
      {/* Header */}
      <header className="fixed top-0 w-full z-50 bg-background/60 backdrop-blur-xl">
        <div className="flex justify-between items-center max-w-2xl mx-auto px-6 h-16">
          <h1
            className="text-xl font-bold tracking-tighter text-on-surface font-headline cursor-pointer"
            onClick={() => navigate("/")}
          >
            BRIVVA
          </h1>
          <button
            className="text-on-surface-variant hover:text-on-surface transition-colors p-2"
            onClick={() => setShowSettings(!showSettings)}
          >
            <Settings2 className="w-4 h-4" />
          </button>
        </div>
      </header>

      <main className="max-w-2xl mx-auto px-6 pt-24 pb-16 space-y-6">
        {youtubeJustConnected && (
          <div className="bg-success/10 text-success px-4 py-2.5 rounded-xl font-label text-sm">
            YouTube account connected
          </div>
        )}

        {/* ── Settings drawer (YouTube, Voices) ─── */}
        {showSettings && (
          <div className="space-y-4 bg-surface-container-low rounded-xl p-5">
            <div className="flex items-center justify-between">
              <span className="text-xs font-label font-bold uppercase tracking-widest text-on-surface-variant">
                Settings
              </span>
              <button
                className="text-on-surface-variant hover:text-on-surface p-1"
                onClick={() => setShowSettings(false)}
              >
                <X className="w-3.5 h-3.5" />
              </button>
            </div>

            {/* YouTube */}
            <div>
              <span className="text-on-surface-variant text-xs font-label block mb-2">
                YouTube Account
              </span>
              {user?.youtube_connected ? (
                <div className="flex items-center gap-2">
                  <Youtube className="w-4 h-4 text-[#ff0000]" />
                  <span className="text-on-surface font-label text-sm">
                    {user.youtube_channel_name}
                  </span>
                  <span className="text-[10px] font-label font-bold uppercase tracking-widest text-success bg-success/10 px-2 py-0.5 rounded">
                    Connected
                  </span>
                </div>
              ) : (
                <a
                  href={api.youtubeAuthUrl(userId)}
                  className="inline-flex items-center gap-2 bg-surface-container-high hover:bg-surface-bright text-on-surface px-4 py-2 rounded-lg font-label text-sm transition-colors"
                >
                  <Youtube className="w-3.5 h-3.5" />
                  Connect YouTube
                </a>
              )}
            </div>

            {/* Voices */}
            <div>
              <span className="text-on-surface-variant text-xs font-label block mb-2">
                Saved Voices
              </span>
              {voices.length === 0 ? (
                <p className="text-on-surface-variant/60 text-xs">
                  No saved voices. Record one when broadcasting.
                </p>
              ) : (
                <div className="space-y-1.5">
                  {voices.map((v) => (
                    <div
                      key={v.id}
                      className={cn(
                        "flex items-center gap-3 px-3 py-2 rounded-lg cursor-pointer transition-colors text-sm",
                        selectedVoice === v.id
                          ? "bg-primary-container/20"
                          : "bg-surface-container-high hover:bg-surface-bright"
                      )}
                      onClick={() => setSelectedVoice(v.id)}
                    >
                      <Mic className="w-3.5 h-3.5 text-primary" />
                      <span className="text-on-surface font-label flex-1 truncate">
                        {v.name}
                      </span>
                      <button
                        className="text-on-surface-variant hover:text-error transition-colors p-0.5"
                        onClick={(e) => {
                          e.stopPropagation();
                          handleDeleteVoice(v.id);
                        }}
                      >
                        <Trash2 className="w-3 h-3" />
                      </button>
                    </div>
                  ))}
                </div>
              )}
            </div>
          </div>
        )}

        {/* ── Session Title + Language (compact row) ─── */}
        <div className="flex gap-3">
          <input
            className="flex-1 min-w-0 bg-surface-container-highest border-none rounded-xl px-4 py-3 text-on-surface placeholder:text-on-surface-variant/50 focus:ring-2 focus:ring-primary/50 transition-all font-label outline-none"
            placeholder="Session title"
            value={title}
            onChange={(e) => setTitle(e.target.value)}
          />
          <div className="relative shrink-0">
            <select
              className="appearance-none bg-surface-container-highest border-none rounded-xl px-4 py-3 text-on-surface font-label focus:ring-2 focus:ring-primary/50 transition-all outline-none pr-9 cursor-pointer"
              value={sourceLang}
              onChange={(e) => setSourceLang(e.target.value)}
            >
              {api.LANGS.map((l) => (
                <option key={l.code} value={l.code}>
                  {l.flag} {l.label}
                </option>
              ))}
            </select>
            <ChevronDown className="absolute right-3 top-1/2 -translate-y-1/2 w-4 h-4 text-on-surface-variant pointer-events-none" />
          </div>
        </div>

        {/* ── Destinations ─────────────────────────── */}
        <div className="space-y-3">
          {/* Added destination cards */}
          {destinations.map((dest) => (
            <DestinationCard
              key={dest.uid}
              dest={dest}
              sourceLang={sourceLang}
              savedCreds={savedCreds}
              user={user}
              privacyStatus={privacyStatus}
              onPrivacyChange={setPrivacyStatus}
              onUpdate={(patch) => updateDestination(dest.uid, patch)}
              onRemove={() => removeDestination(dest.uid)}
            />
          ))}

          {/* Add destination button + picker */}
          <div className="relative" ref={pickerRef}>
            {!pickerOpen ? (
              <div className="flex gap-2">
                <button
                  className="flex-1 flex items-center justify-center gap-2 py-3 rounded-xl border-2 border-dashed border-outline-variant/20 text-on-surface-variant hover:border-primary/40 hover:text-primary transition-all font-label text-sm"
                  onClick={() => setPickerOpen(true)}
                >
                  <Plus className="w-4 h-4" />
                  Add destination
                </button>
                <button
                  className="flex items-center gap-2 px-4 py-3 rounded-xl border-2 border-dashed border-outline-variant/20 text-on-surface-variant hover:border-primary/40 hover:text-primary transition-all"
                  onClick={() => {
                    const url = prompt("Paste RTMP URL");
                    if (url) handleMagicPaste(url);
                  }}
                  title="Paste RTMP URL"
                >
                  <Clipboard className="w-4 h-4" />
                </button>
              </div>
            ) : (
              <div className="bg-surface-container-low rounded-xl overflow-hidden">
                {/* Paste bar inside picker */}
                <div className="p-3 border-b border-outline-variant/10">
                  <input
                    className="w-full bg-surface-container-highest border-none rounded-lg px-3 py-2 text-on-surface placeholder:text-on-surface-variant/40 focus:ring-2 focus:ring-primary/50 transition-all font-label text-sm outline-none"
                    placeholder="Paste RTMP URL to auto-detect..."
                    value={magicPaste}
                    onChange={(e) => handleMagicPaste(e.target.value)}
                    autoFocus
                  />
                </div>

                {/* Region groups */}
                {REGION_GROUPS.map((group) => {
                  const platforms = api.PLATFORMS.filter(
                    (p) => p.region === group.key
                  );
                  if (platforms.length === 0) return null;

                  const isExpanded = expandedRegion === group.key;

                  return (
                    <div key={group.key}>
                      <button
                        className="w-full flex items-center gap-3 px-4 py-3 hover:bg-surface-container-high transition-colors text-left"
                        onClick={() =>
                          setExpandedRegion(isExpanded ? null : group.key)
                        }
                      >
                        <span className="text-sm">{group.icon}</span>
                        <span className="text-on-surface font-label text-sm flex-1">
                          {group.label}
                        </span>
                        <span className="text-on-surface-variant/40 text-xs font-label">
                          {platforms.length}
                        </span>
                        {isExpanded ? (
                          <ChevronDown className="w-3.5 h-3.5 text-on-surface-variant" />
                        ) : (
                          <ChevronRight className="w-3.5 h-3.5 text-on-surface-variant" />
                        )}
                      </button>

                      {isExpanded && (
                        <div className="pb-2">
                          {platforms.map((p) => {
                            const autoLang = api.PLATFORM_LANG[p.id];
                            const disabled =
                              autoLang !== null && autoLang === sourceLang;

                            return (
                              <button
                                key={p.id}
                                disabled={disabled}
                                className={cn(
                                  "w-full flex items-center gap-3 px-4 pl-10 py-2.5 text-left transition-colors",
                                  disabled
                                    ? "text-on-surface-variant/30 cursor-not-allowed"
                                    : "text-on-surface hover:bg-surface-container-high cursor-pointer"
                                )}
                                onClick={() => addDestination(p.id)}
                              >
                                <PlatformIcon
                                  id={p.id}
                                  className="w-4 h-4 shrink-0"
                                />
                                <span className="font-label text-sm flex-1">
                                  {p.label}
                                </span>
                                {autoLang && !disabled && (
                                  <span className="text-xs text-on-surface-variant/50">
                                    {api.langFlag(autoLang)}{" "}
                                    {api.langLabel(autoLang)}
                                  </span>
                                )}
                                {disabled && (
                                  <span className="text-[10px] text-on-surface-variant/30 font-label">
                                    same as source
                                  </span>
                                )}
                              </button>
                            );
                          })}
                        </div>
                      )}
                    </div>
                  );
                })}

                {/* Close */}
                <button
                  className="w-full py-2.5 text-on-surface-variant text-xs font-label hover:bg-surface-container-high transition-colors border-t border-outline-variant/10"
                  onClick={() => {
                    setPickerOpen(false);
                    setExpandedRegion(null);
                  }}
                >
                  Cancel
                </button>
              </div>
            )}
          </div>
        </div>

        {/* Error */}
        {error && (
          <p className="text-error text-sm font-label bg-error-container/20 px-4 py-2.5 rounded-xl">
            {error}
          </p>
        )}

        {/* YouTube privacy (only if YouTube destination added) */}
        {hasYoutubeDest && (
          <div className="flex items-center gap-3 bg-surface-container-low rounded-xl px-4 py-3">
            <Youtube className="w-4 h-4 text-[#ff0000]" />
            <span className="text-on-surface-variant text-xs font-label flex-1">
              YouTube privacy
            </span>
            <div className="relative">
              <select
                className="appearance-none bg-surface-container-highest border-none rounded-lg px-3 py-1.5 text-on-surface font-label text-sm focus:ring-2 focus:ring-primary/50 outline-none pr-7 cursor-pointer"
                value={privacyStatus}
                onChange={(e) => setPrivacyStatus(e.target.value)}
              >
                <option value="public">Public</option>
                <option value="unlisted">Unlisted</option>
                <option value="private">Private</option>
              </select>
              <ChevronDown className="absolute right-2 top-1/2 -translate-y-1/2 w-3 h-3 text-on-surface-variant pointer-events-none" />
            </div>
          </div>
        )}

        {/* Go Live */}
        <button
          className={cn(
            "w-full py-4 rounded-xl font-headline font-extrabold text-lg uppercase tracking-tight transition-all",
            destinations.length > 0
              ? "monolith-gradient text-white hover:scale-[0.99] active:scale-[0.97] shadow-xl"
              : "bg-surface-container-high text-on-surface-variant cursor-not-allowed"
          )}
          onClick={handleCreateSession}
          disabled={creating || destinations.length === 0}
        >
          <span className="flex items-center justify-center gap-2">
            <Radio className="w-5 h-5" />
            {creating
              ? "Creating..."
              : destinations.length === 0
                ? "Add a destination to go live"
                : `Go Live${destinations.length > 1 ? ` · ${destinations.length} destinations` : ""}`}
          </span>
        </button>

        {/* ── Past Sessions ───────────────────────── */}
        {sessions.length > 0 && (
          <div className="pt-6">
            <span className="text-xs font-label font-bold uppercase tracking-widest text-on-surface-variant block mb-3">
              Recent Sessions
            </span>
            <div className="space-y-1.5">
              {sessions.map((s) => (
                <div
                  key={s.id}
                  className="flex items-center gap-3 bg-surface-container-high hover:bg-surface-bright px-4 py-3 rounded-lg cursor-pointer transition-colors"
                  onClick={() => navigate(`/session/${s.id}`)}
                >
                  <span className="text-on-surface font-label text-sm flex-1 truncate">
                    {s.title}
                  </span>
                  <span
                    className={cn(
                      "text-[10px] font-label font-bold uppercase tracking-widest px-2 py-0.5 rounded shrink-0",
                      s.status === "live"
                        ? "text-success bg-success/10"
                        : s.status === "ended"
                          ? "text-on-surface-variant bg-surface-container-highest"
                          : "text-primary bg-primary/10"
                    )}
                  >
                    {s.status}
                  </span>
                </div>
              ))}
            </div>
          </div>
        )}
      </main>
    </div>
  );
}

// ── Destination Card ──────────────────────────────────

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

function DestinationCard({
  dest,
  sourceLang,
  savedCreds,
  user: _user,
  privacyStatus: _privacyStatus,
  onPrivacyChange: _onPrivacyChange,
  onUpdate,
  onRemove,
}: {
  dest: Destination;
  sourceLang: string;
  savedCreds: Record<string, api.PlatformCredential>;
  user: api.UserInfo | null;
  privacyStatus: string;
  onPrivacyChange: (v: string) => void;
  onUpdate: (patch: Partial<Destination>) => void;
  onRemove: () => void;
}) {
  const [expanded, setExpanded] = useState(() => {
    const p = api.PLATFORMS.find((x) => x.id === dest.platform);
    // Auto-expand if needs manual config and no saved creds
    return !!(p && !p.auto && !savedCreds[dest.platform]);
  });

  const platform = api.PLATFORMS.find((p) => p.id === dest.platform);
  const fixedLang = api.PLATFORM_LANG[dest.platform];
  if (!platform) return null;

  const needsConfig = !platform.auto;
  const hasConfig = !!(dest.rtmp_url || dest.stream_key);
  const hasSavedCreds = !!savedCreds[dest.platform];

  return (
    <div className="bg-surface-container-low rounded-xl overflow-hidden">
      {/* Compact header — always visible */}
      <div className="flex items-center gap-3 px-4 py-3">
        <PlatformIcon id={dest.platform} className="w-4 h-4 text-primary" />
        <span className="font-label font-bold text-on-surface text-sm flex-1 truncate">
          {platform.label}
        </span>

        {/* Language: badge or picker */}
        {fixedLang ? (
          <span className="text-[10px] font-label font-bold uppercase tracking-widest text-primary bg-primary/10 px-2 py-0.5 rounded">
            {api.langFlag(dest.lang)} {api.langLabel(dest.lang)}
          </span>
        ) : (
          <div className="relative">
            <select
              className="appearance-none bg-surface-container-highest border-none rounded px-2.5 py-1 text-on-surface font-label text-xs focus:ring-2 focus:ring-primary/50 outline-none pr-6 cursor-pointer"
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
        )}

        {/* Status dot */}
        {needsConfig && (
          <span
            className={cn(
              "w-2 h-2 rounded-full shrink-0",
              hasConfig || hasSavedCreds ? "bg-success" : "bg-error/60"
            )}
            title={hasConfig || hasSavedCreds ? "Configured" : "Needs stream key"}
          />
        )}

        {/* Expand toggle for manual platforms */}
        {needsConfig && (
          <button
            className="text-on-surface-variant hover:text-on-surface p-1 transition-colors"
            onClick={() => setExpanded(!expanded)}
          >
            {expanded ? (
              <ChevronDown className="w-3.5 h-3.5" />
            ) : (
              <ChevronRight className="w-3.5 h-3.5" />
            )}
          </button>
        )}

        <button
          className="text-on-surface-variant hover:text-error transition-colors p-1"
          onClick={onRemove}
        >
          <X className="w-3.5 h-3.5" />
        </button>
      </div>

      {/* Always-on timing + mix controls — apply to every stream */}
      <div className="px-4 pb-3 pt-1 space-y-2">
        <StreamSlider
          label="Output delay"
          valueLabel={`${dest.delay_ms} ms`}
          min={0}
          max={5000}
          step={100}
          value={dest.delay_ms}
          onChange={(v) => onUpdate({ delay_ms: v })}
          hint="Fargate holds the original media this long before emitting, leaving time for STT + translate + TTS."
        />
        <StreamSlider
          label={
            dest.lang === sourceLang ? "Original audio volume" : "Under-voice volume"
          }
          valueLabel={`${Math.round(dest.host_gain * 100)}%`}
          min={0}
          max={100}
          step={5}
          value={Math.round(dest.host_gain * 100)}
          onChange={(v) => onUpdate({ host_gain: v / 100 })}
          hint={
            dest.lang === sourceLang
              ? "100% for source streams — no translation overlay to duck under."
              : "How loud the original voice sits under the translated speech."
          }
        />
      </div>

      {/* Expandable config section */}
      {needsConfig && expanded && (
        <div className="px-4 pb-4 space-y-2">
          {hasSavedCreds && (
            <span className="text-[10px] font-label text-success uppercase tracking-widest">
              Pre-filled from saved credentials
            </span>
          )}
          {platform.settingsUrl && (
            <a
              href={platform.settingsUrl}
              target="_blank"
              rel="noopener noreferrer"
              className="flex items-center gap-1 text-primary text-xs font-label hover:underline"
            >
              Open {platform.label} Settings
              <ExternalLink className="w-3 h-3" />
            </a>
          )}
          <p className="text-on-surface-variant/60 text-xs leading-relaxed">
            {platform.help}
          </p>
          <div className="grid grid-cols-1 sm:grid-cols-2 gap-2">
            {!platform.keyOnly && (
              <input
                className="bg-surface-container-highest border-none rounded-lg px-3 py-2 text-on-surface placeholder:text-on-surface-variant/50 focus:ring-2 focus:ring-primary/50 transition-all font-label text-sm outline-none"
                placeholder="Server URL"
                value={dest.rtmp_url}
                onChange={(e) => onUpdate({ rtmp_url: e.target.value })}
              />
            )}
            <input
              className="bg-surface-container-highest border-none rounded-lg px-3 py-2 text-on-surface placeholder:text-on-surface-variant/50 focus:ring-2 focus:ring-primary/50 transition-all font-label text-sm outline-none"
              placeholder="Stream Key"
              type="password"
              value={dest.stream_key}
              onChange={(e) => onUpdate({ stream_key: e.target.value })}
            />
          </div>
        </div>
      )}
    </div>
  );
}
