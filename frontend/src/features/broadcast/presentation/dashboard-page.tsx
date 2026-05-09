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
import { SOURCE_LANGS, type SourceLang } from "./source-lang-picker";
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

/**
 * Returns a user-visible error if a destination can't be shipped as-is,
 * otherwise null. Mirrors the server-side expectations in POST /api/sessions:
 *   - source lang can't also be a destination lang
 *   - YouTube needs an OAuth-connected account (server will auto-create the
 *     broadcast, so rtmp_url/stream_key are OPTIONAL on the client — YouTube
 *     is treated as auto-fillable here)
 *   - platforms without `.auto` need both rtmp_url + stream_key
 */
// Paste-creds platforms: Grip has a Seller API path that would auto-fill, but
// for the May 10 launch we treat them as manual-only — the host pastes RTMP
// url + stream key, the FE persists them via /auth/grip or /auth/tiktok.
const PASTE_CREDS_PLATFORMS = new Set(["grip", "tiktok"]);

function destinationError(
  dest: Destination,
  sourceLang: string,
  user: api.UserInfo | null,
): string | null {
  if (dest.lang === sourceLang) {
    return `${api.langLabel(dest.lang)} is your source language — change or remove this destination`;
  }
  const p = api.PLATFORMS.find((x) => x.id === dest.platform);
  if (dest.platform === "youtube") {
    if (!user?.youtube_connected) return "Connect your YouTube account first";
    return null; // rtmp_url/stream_key are auto-filled server-side
  }
  if (!p) return null;
  const pasteOnly = PASTE_CREDS_PLATFORMS.has(dest.platform);
  if (!p.auto || pasteOnly) {
    if (!p.keyOnly && !dest.rtmp_url) return `Enter server URL for ${p.label}`;
    if (!dest.stream_key) return `Enter stream key for ${p.label}`;
  }
  return null;
}

function isLikelyMobileHost(): boolean {
  if (typeof navigator === "undefined") return false;
  return /Android|iPhone|iPad|iPod/i.test(navigator.userAgent);
}

function browserCompatibilityWarning(): string | null {
  if (typeof navigator === "undefined") return null;
  const ua = navigator.userAgent;
  const isFirefox = /Firefox|FxiOS/i.test(ua);
  const isSafari = /Safari/i.test(ua) && !/Chrome|Chromium|CriOS|Edg|OPR/i.test(ua);
  if (isFirefox) {
    return "Firefox uses VP8 for WebRTC on Brivva to avoid H.264 startup issues. Keep the tab foregrounded and watch provider health after Record.";
  }
  if (isSafari) {
    return "Safari supports WebRTC/H.264, but camera/network behavior differs from Chromium. Keep the tab foregrounded and watch provider health after Record.";
  }
  return null;
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
  const [translationTerms, setTranslationTerms] = useState("");
  const [magicPaste, setMagicPaste] = useState("");
  const [gripFreshConfirmed, setGripFreshConfirmed] = useState(false);
  const [quoteSessionId, setQuoteSessionId] = useState<string | null>(null);
  // Surfaced when POST /api/sessions returns 400 voice_language_mismatch
  // even though the FE's reactive state didn't catch it (stale voice cache
  // after a race). Forces the banner on so the host has a path forward.
  const [postMismatch, setPostMismatch] = useState(false);
  const mobileHostWarning = isLikelyMobileHost();
  const browserCompatibilityNotice = browserCompatibilityWarning();

  // Progressive disclosure
  const [pickerOpen, setPickerOpen] = useState(false);
  const [showSettings, setShowSettings] = useState(false);
  const pickerRef = useRef<HTMLDivElement>(null);
  const voiceSectionRef = useRef<HTMLDivElement>(null);

  // Incrementing counter → forwarded to <YourVoiceSection> as
  // `recordRequestKey`. Bumping it imperatively opens the re-record UI from
  // the mismatch banner CTA.
  const [recordRequestKey, setRecordRequestKey] = useState(0);

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
      if (u.active_voice_id) setSelectedVoice(u.active_voice_id);
      const credMap: Record<string, api.PlatformCredential> = {};
      for (const cred of c.credentials) {
        // Grip stream keys are one-shot per broadcast (AWS IVS rejects a
        // duplicate publisher, causing silent mid-stream failure). Legacy
        // rows may still exist in platform_credentials from before the
        // save path was removed — ignore them so the destination card
        // never pre-fills a stale Grip key.
        if (cred.platform === "grip") continue;
        credMap[cred.platform] = cred;
      }
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
      } catch (error) {
        // Workers may not have shipped onboarding_completed_at yet — let the
        // dashboard render normally so the demo path keeps working.
        console.warn(
          "dashboard: failed to fetch onboarding state — staying on dashboard",
          { userId, error: (error as Error).message },
        );
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

    if (platformId === "grip") setGripFreshConfirmed(false);
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
      prev.map((d) => {
        if (d.uid !== uid) return d;
        if (d.platform === "grip" && patch.stream_key !== undefined) {
          setGripFreshConfirmed(false);
        }
        // When the target language flips and the user hadn't hand-tuned
        // timing, re-apply the curated defaults. Otherwise switching to
        // Passthrough would leave a 2500ms delay + 0.2 gain on what is now
        // a raw-audio stream.
        const next = { ...d, ...patch };
        if (patch.lang && patch.lang !== d.lang) {
          const prior = streamDefault(sourceLang, d.lang);
          const userHadTuned =
            prior.delay_ms !== d.delay_ms || prior.host_gain !== d.host_gain;
          if (!userHadTuned) {
            const tuned = streamDefault(sourceLang, patch.lang);
            next.delay_ms = tuned.delay_ms;
            next.host_gain = tuned.host_gain;
          }
        }
        return next;
      }),
    );
  };

  const handleMagicPaste = (value: string) => {
    setMagicPaste(value);
    const detected = api.detectPlatform(value);
    if (detected) {
      const destLang = pickDestinationLang(sourceLang);
      const tuned = streamDefault(sourceLang, destLang);
      if (detected.platform === "grip") setGripFreshConfirmed(false);
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
        // Paste-creds platforms (Grip, TikTok) are marked auto=true so the
        // picker shows a checkmark for Grip's future Seller API path, but the
        // FE still needs to ship the pasted rtmp/stream_key with the session
        // so Workers can insert a manual stream row when the Seller API path
        // isn't available.
        const pasteOnly = PASTE_CREDS_PLATFORMS.has(d.platform);
        if (p?.auto && !pasteOnly) return base;
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
        translation_terms: translationTerms.trim() || undefined,
      });

      if (result.errors?.length) setError(result.errors.join("; "));
      // Open the pre-stream quote modal before sending the host into the
      // setup flow. They confirm or cancel; cancel leaves the freshly
      // created session in place so they can resume from /dashboard later.
      setQuoteSessionId(result.session.id);
    } catch (e: unknown) {
      const msg = e instanceof Error ? e.message : "Failed to create session";
      // If Workers rejects with the strict enrollment guard, surface the
      // banner rather than a raw "API 400: voice_language_mismatch" toast.
      if (msg.includes("voice_language_mismatch")) {
        setPostMismatch(true);
        setError("");
      } else {
        setError(msg);
      }
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
  const hasGripDest = destinations.some((d) => d.platform === "grip");
  const hasTranslatedDestination = destinations.some(
    (d) => d.lang !== "pass" && d.lang !== sourceLang,
  );
  const missingFreshGripConfirmation = hasGripDest && !gripFreshConfirmed;
  // Disable Go Live when any destination has a client-side validation error.
  // Prevents the "stream row created with NULL rtmp_url → session hangs"
  // production bug from reaching the backend a second time (Apr 19, 2026).
  const hasInvalidDestination = destinations.some(
    (d) => destinationError(d, sourceLang, user) !== null,
  );

  // Voice/source language mismatch. The voice clone can only synthesize
  // cleanly in the enrollment language (ElevenLabs cross-lingual steering
  // works on the TARGET side via `language_code`, not the source side). If
  // the host cloned in English but starts a Korean session, the clone
  // re-synthesizes with the wrong accent (Apr 2026 regression). We gate Go
  // Live here so the backend's strict guard never has to reject after a
  // round-trip. A null `voice.source_lang` (legacy row) is also a mismatch —
  // we can't prove it matches, so force a re-record.
  const voice = voices[0] ?? null;
  const voiceLang = voice?.source_lang ?? null;
  const reactiveVoiceLangMismatch = voice !== null && voiceLang !== sourceLang;
  const voiceLangMismatch = hasTranslatedDestination && (reactiveVoiceLangMismatch || postMismatch);
  const voiceLangKnown =
    voiceLang !== null && (SOURCE_LANGS as readonly string[]).includes(voiceLang);
  const voiceLangLabel = voiceLangKnown ? api.langLabel(voiceLang!) : "an unknown language";
  const sourceLangLabel = api.langLabel(sourceLang);

  const handleRerecordFromBanner = () => {
    // Open the settings drawer (YourVoiceSection lives inside it), then
    // bump `recordRequestKey` so the section imperatively enters recording
    // mode + seeds its picker from `initialSourceLang` (= sourceLang).
    setShowSettings(true);
    setRecordRequestKey((k) => k + 1);
    // Defer the scroll so the drawer's DOM has mounted.
    setTimeout(() => {
      voiceSectionRef.current?.scrollIntoView({ behavior: "smooth", block: "center" });
    }, 0);
  };

  const handleSwapSessionSourceToVoiceLang = () => {
    if (voiceLangKnown) {
      setSourceLang(voiceLang!);
      setPostMismatch(false);
    }
  };

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
            <div ref={voiceSectionRef}>
              <YourVoiceSection
                userId={userId}
                voice={voices[0] ?? null}
                defaultName={user?.youtube_channel_name ?? "My voice"}
                onChange={(next) => {
                  setVoices([next]);
                  setSelectedVoice(next.id);
                  // Voice row just changed — clear any sticky server-side
                  // rejection so the banner drops if the new row matches.
                  setPostMismatch(false);
                }}
                initialSourceLang={sourceLang as SourceLang}
                recordRequestKey={recordRequestKey}
              />
            </div>

            {/* Account management (hard-delete + full voice clone list) */}
            <div className="pt-2 border-t border-surface-container-high">
              <button
                onClick={() => navigate("/settings")}
                className="text-on-surface-variant hover:text-on-surface font-label text-sm transition-colors"
                data-testid="open-settings"
              >
                Manage account →
              </button>
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
              data-testid="session-source-lang"
              className="appearance-none bg-surface-container-highest border-none rounded-xl px-4 py-3 text-on-surface font-label focus:ring-2 focus:ring-primary/50 transition-all outline-none pr-9 cursor-pointer"
              value={sourceLang}
              onChange={(e) => {
                setSourceLang(e.target.value);
                setPostMismatch(false);
              }}
            >
              {api.LANGS.filter((l) => !api.isPassthroughLang(l.code)).map((l) => (
                <option key={l.code} value={l.code}>
                  {l.flag} {l.label}
                </option>
              ))}
            </select>
            <ChevronDown className="absolute right-3 top-1/2 -translate-y-1/2 w-4 h-4 text-on-surface-variant pointer-events-none" />
          </div>
        </div>

        {/* ── Voice / source-lang mismatch banner ──── */}
        {voiceLangMismatch && (
          <div
            role="alert"
            data-testid="voice-lang-mismatch-banner"
            className="bg-error-container/20 text-on-error-container rounded-xl px-4 py-3 space-y-2"
          >
            <p className="text-sm font-label text-on-surface">
              Your voice clone was recorded in {voiceLangLabel}, but this
              session's source is {sourceLangLabel}. Re-record the clone or
              change the session source language.
            </p>
            <div className="flex flex-wrap gap-2">
              <button
                type="button"
                className="bg-surface-container-high hover:bg-surface-bright text-on-surface px-3 py-1.5 rounded-lg font-label text-xs transition-colors"
                onClick={handleRerecordFromBanner}
              >
                Re-record voice
              </button>
              {voiceLangKnown && (
                <button
                  type="button"
                  className="bg-surface-container-high hover:bg-surface-bright text-on-surface px-3 py-1.5 rounded-lg font-label text-xs transition-colors"
                  onClick={handleSwapSessionSourceToVoiceLang}
                >
                  Change session source to {voiceLangLabel}
                </button>
              )}
            </div>
          </div>
        )}

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
              userId={userId}
              privacyStatus={privacyStatus}
              onPrivacyChange={setPrivacyStatus}
              onUpdate={(patch) => updateDestination(dest.uid, patch)}
              onRemove={() => removeDestination(dest.uid)}
              onCredentialSaved={(cred) =>
                setSavedCreds((prev) => ({ ...prev, [cred.platform]: cred }))
              }
              validationError={destinationError(dest, sourceLang, user)}
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

        {hasGripDest && (
          <label className="flex gap-3 rounded-xl bg-warning/10 px-4 py-3 text-xs font-label text-on-surface-variant">
            <input
              type="checkbox"
              className="mt-0.5 accent-primary"
              checked={gripFreshConfirmed}
              onChange={(e) => setGripFreshConfirmed(e.target.checked)}
            />
            <span>
              <span className="block font-bold text-on-surface">
                Grip key check
              </span>
              I pasted a fresh, current-session Grip stream key from Grip admin.
              Reused keys can silently fail after a few seconds.
            </span>
          </label>
        )}

        {/* Translation hints */}
        <div className="bg-surface-container-low rounded-xl px-4 py-3 space-y-2">
          <label className="text-on-surface text-sm font-label font-bold block">
            Product terms for translation <span className="text-on-surface-variant font-normal">optional</span>
          </label>
          <p className="text-on-surface-variant text-xs font-label leading-relaxed">
            Add brand names, product names, promo codes, prices, sizing words, or phrases the host will say. This helps the live translator keep important commerce terms accurate instead of guessing.
          </p>
          <textarea
            className="w-full min-h-24 bg-surface-container-highest border-none rounded-lg px-3 py-2 text-on-surface placeholder:text-on-surface-variant/40 focus:ring-2 focus:ring-primary/50 transition-all font-label text-sm outline-none resize-y"
            placeholder="Example: Brivva Pro serum, 1+1 bundle, SUMMER20, ₩29,900, collagen ampoule"
            value={translationTerms}
            maxLength={2000}
            onChange={(e) => setTranslationTerms(e.target.value)}
          />
          <div className="text-right text-[10px] font-label text-on-surface-variant">
            {translationTerms.length}/2000
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

        {browserCompatibilityNotice && (
          <div className="rounded-xl bg-warning-container/20 px-4 py-3 text-xs font-label text-on-surface-variant">
            <span className="font-bold text-on-surface">Browser compatibility notice.</span>{" "}
            {browserCompatibilityNotice}
          </div>
        )}

        {mobileHostWarning && (
          <div className="rounded-xl bg-warning-container/20 px-4 py-3 text-xs font-label text-on-surface-variant">
            <span className="font-bold text-on-surface">Desktop recommended for hosting.</span>{" "}
            Mobile browsers may pause camera/mic when backgrounded, locked, or switching networks.
          </div>
        )}

        {/* Go Live */}
        <button
          className={cn(
            "w-full py-4 rounded-xl font-headline font-extrabold text-lg uppercase tracking-tight transition-all",
            destinations.length > 0 &&
              !hasInvalidDestination &&
              !voiceLangMismatch &&
              !missingFreshGripConfirmation
              ? "monolith-gradient text-white hover:scale-[0.99] active:scale-[0.97] shadow-xl"
              : "bg-surface-container-high text-on-surface-variant cursor-not-allowed"
          )}
          onClick={handleCreateSession}
          disabled={
            creating ||
            destinations.length === 0 ||
            hasInvalidDestination ||
            voiceLangMismatch ||
            missingFreshGripConfirmation
          }
        >
          <span className="flex items-center justify-center gap-2">
            <Radio className="w-5 h-5" />
            {creating
              ? "Creating..."
              : destinations.length === 0
                ? "Add a destination to go live"
                : hasInvalidDestination
                  ? "Fix destination errors to go live"
                  : voiceLangMismatch
                    ? "Fix voice language to go live"
                    : missingFreshGripConfirmation
                      ? "Confirm fresh Grip key to go live"
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

