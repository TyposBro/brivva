import { useEffect, useState, useCallback, useRef } from "react";
import { useNavigate, useSearchParams } from "react-router-dom";
import {
  Plus,
  Radio,
  X,
  Youtube,
  ChevronDown,
  Settings2,
  Clipboard,
} from "lucide-react";
import { cn } from "../../../core/cn";
import * as api from "../data/api-client";
import { SignInGate } from "../../../shared/auth/sign-in-gate";
import { useAuth } from "../../../shared/auth/use-auth";
import { streamDefault } from "../../../core/config/stream-defaults";
import { readDefaultTargetLang } from "../../../core/config/default-target-lang";
import { fetchOnboardingState } from "../data/onboarding-api";
import {
  DestinationCard,
  PlatformIcon,
  type Destination,
} from "./dashboard-destination-card";
import { YourVoiceSection } from "./your-voice-section";
import { QuoteModal } from "./quote-modal";

// ── Component ──────────────────────────────────────────

export default function DashboardPage() {
  return (
    <SignInGate>
      <DashboardInner />
    </SignInGate>
  );
}

// Per-destination target language defaults to the user's onboarding pick.
// Falls back to a sane "not the source" choice if the stored default would
// equal the source language.
function pickDestinationLang(sourceLang: string): string {
  const stored = readDefaultTargetLang();
  if (stored && stored !== sourceLang) return stored;
  return sourceLang === "en" ? "ja" : "en";
}

function DashboardInner() {
  const navigate = useNavigate();
  const [searchParams] = useSearchParams();
  // SignInGate guarantees we only mount when userId is set.
  const userId = useAuth().userId!;

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
  const [quoteSessionId, setQuoteSessionId] = useState<string | null>(null);

  // Progressive disclosure
  const [pickerOpen, setPickerOpen] = useState(false);
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

  // Bounce users into the onboarding wizard until Workers reports
  // onboarding_completed_at. The fetch lives here (not in a global gate) so
  // the redirect happens after the SignInGate has resolved.
  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const state = await fetchOnboardingState(userId);
        if (cancelled) return;
        if (state.onboardingCompletedAt === null) {
          navigate("/onboarding", { replace: true });
        }
      } catch {
        // Workers may not have shipped onboarding_completed_at yet — let the
        // dashboard render normally so the demo path keeps working.
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [navigate, userId]);

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

    const lang = pickDestinationLang(sourceLang);
    const cred = savedCreds[platformId];
    const tuned = streamDefault(sourceLang, lang);

    setDestinations((prev) => [
      ...prev,
      {
        uid: crypto.randomUUID(),
        platform: platformId,
        lang,
        rtmp_url: cred?.rtmp_url ?? platform.defaultRtmp ?? "",
        stream_key: cred?.stream_key ?? "",
        delay_ms: tuned.delay_ms,
        host_gain: tuned.host_gain,
      },
    ]);
    setPickerOpen(false);
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
      const destLang = pickDestinationLang(sourceLang);
      const tuned = streamDefault(sourceLang, destLang);
      setDestinations((prev) => [
        ...prev,
        {
          uid: crypto.randomUUID(),
          platform: detected.platform,
          lang: destLang,
          rtmp_url: detected.rtmpUrl,
          stream_key: detected.streamKey,
          delay_ms: tuned.delay_ms,
          host_gain: tuned.host_gain,
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
      // Open the pre-stream quote modal before sending the host into the
      // setup flow. They confirm or cancel; cancel leaves the freshly
      // created session in place so they can resume from /dashboard later.
      setQuoteSessionId(result.session.id);
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : "Failed to create session");
    } finally {
      setCreating(false);
    }
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

            {/* Voice — one clone per user, Workers upserts on POST /api/voices. */}
            <YourVoiceSection
              userId={userId}
              voice={voices[0] ?? null}
              defaultName={user?.youtube_channel_name ?? "My voice"}
              onChange={(next) => {
                setVoices([next]);
                setSelectedVoice(next.id);
              }}
            />
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

                {/* Flat platform grid — language is chosen per destination
                    on the destination card, not gated by platform. */}
                <div className="grid grid-cols-2 gap-1 p-2">
                  {api.PLATFORMS.map((p) => (
                    <button
                      key={p.id}
                      className="flex items-center gap-3 px-3 py-2.5 rounded-lg text-on-surface hover:bg-surface-container-high cursor-pointer transition-colors text-left"
                      onClick={() => addDestination(p.id)}
                    >
                      <PlatformIcon id={p.id} className="w-4 h-4 shrink-0 text-primary" />
                      <span className="font-label text-sm flex-1 truncate">
                        {p.label}
                      </span>
                    </button>
                  ))}
                </div>

                {/* Close */}
                <button
                  className="w-full py-2.5 text-on-surface-variant text-xs font-label hover:bg-surface-container-high transition-colors border-t border-outline-variant/10"
                  onClick={() => setPickerOpen(false)}
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

      {quoteSessionId && (
        <QuoteModal
          sessionId={quoteSessionId}
          onCancel={() => setQuoteSessionId(null)}
          onConfirm={() => {
            const id = quoteSessionId;
            setQuoteSessionId(null);
            navigate(`/session/${id}/setup`);
          }}
        />
      )}
    </div>
  );
}

