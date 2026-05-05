import { useEffect, useRef, useState, useCallback } from "react";
import { useNavigate } from "react-router-dom";
import { ArrowLeft, Check, Copy, ExternalLink, Loader2 } from "lucide-react";
import { useHostSession } from "./use-host-session";
import type { MediaDiagnostics, ProviderHealthNotice } from "./reducer";
import { AudioRecorder } from "./audio-recorder";
import { LatencyDashboard } from "./latency-dashboard";
import { VoiceSetupCard } from "./voice-setup-card";
import { SOURCE_LANGS, type SourceLang } from "./source-lang-picker";
import { cn } from "../../../core/cn";
import * as api from "../data/api-client";

export interface BroadcastViewProps {
  // Required — every broadcast is anchored to a real session row.
  // Brivva is host-only: see vision.md "What's Intentionally Not In The
  // Product".
  sessionId: string;
  sourceLang: string;
  userId: string;
  /** When true the voice-setup phase auto-skips on connect (used after the
   *  user already cloned a voice on the setup page). */
  autoSkipVoice?: boolean;
}

export function BroadcastView({
  sessionId,
  sourceLang,
  userId,
  autoSkipVoice = false,
}: BroadcastViewProps) {
  const navigate = useNavigate();
  const {
    status,
    liveTranscript,
    utterances,
    translations,
    analyser,
    error,
    timings,
    mediaDiagnostics,
    connectionIssue,
    providerHealth,
    videoRef,
    connectSession,
    startRecording,
    stopRecording,
    closeSession,
    startVoiceRecording,
    stopVoiceRecording,
    skipVoiceSetup,
    voiceElapsedSec,
    voiceIsRecording,
    voiceMinSec,
    voiceMaxSec,
    setActiveTargetLangs,
  } = useHostSession();

  const listRef = useRef<HTMLDivElement>(null);
  const createdRef = useRef(false);
  const autoSkippedRef = useRef(false);

  const [session, setSession] = useState<api.Session | null>(null);
  const [streams, setStreams] = useState<api.StreamInfo[]>([]);

  const loadSession = useCallback(async () => {
    try {
      const data = await api.getSession(sessionId);
      setSession(data.session);
      setStreams(data.streams);
    } catch (e) {
      console.error("Failed to load session:", e);
    }
  }, [sessionId]);

  useEffect(() => {
    void loadSession();
  }, [loadSession]);

  useEffect(() => {
    if (createdRef.current) return;
    createdRef.current = true;
    void connectSession({ sessionId, sourceLang, userId });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    if (autoSkipVoice && !autoSkippedRef.current && status === "voice_setup") {
      autoSkippedRef.current = true;
      skipVoiceSetup();
    }
  }, [autoSkipVoice, status, skipVoiceSetup]);

  useEffect(() => {
    const langs = streams.map((s) => s.lang).filter((l) => l && l !== sourceLang);
    setActiveTargetLangs([...new Set(langs)]);
  }, [streams, sourceLang, setActiveTargetLangs]);

  useEffect(() => {
    if (listRef.current) listRef.current.scrollTop = listRef.current.scrollHeight;
  }, [utterances, liveTranscript]);

  const handleBack = () => {
    closeSession();
    navigate(`/session/${sessionId}`);
  };

  const isReady = status === "ready" || status === "recording";
  const isRecording = status === "recording";

  return (
    <div className="min-h-screen bg-background">
      <header className="fixed top-0 w-full z-50 bg-background/60 backdrop-blur-xl">
        <div className="flex justify-between items-center max-w-5xl mx-auto px-8 h-16">
          <button
            className="flex items-center gap-2 text-on-surface-variant hover:text-on-surface transition-colors font-label text-sm"
            onClick={handleBack}
          >
            <ArrowLeft className="w-4 h-4" />
            Back
          </button>
          <h1 className="text-xl font-bold tracking-tighter text-on-surface font-headline">
            BRIVVA
          </h1>
          <p className="text-on-surface-variant font-label text-sm truncate max-w-[200px]">
            {session?.title ?? ""}
          </p>
        </div>
      </header>

      <main className="max-w-5xl mx-auto px-8 pt-24 pb-16 space-y-8">
        {error && (
          <div className="bg-error-container/20 text-error px-4 py-2.5 rounded-lg font-label text-sm">
            {error}
          </div>
        )}

        {status === "creating" && (
          <div className="flex items-center gap-3 text-on-surface-variant font-label">
            <Loader2 className="w-4 h-4 animate-spin" />
            Connecting broadcast...
          </div>
        )}

        {connectionIssue && (
          <div className="bg-error-container/20 text-error px-4 py-3 rounded-lg font-label text-sm space-y-1">
            <p className="font-bold">Media connection problem</p>
            <p>{connectionIssue}</p>
            <p className="text-xs text-on-surface-variant">
              For launch tests, use desktop Chrome/Brave, keep the tab visible, and avoid Wi‑Fi/LTE handoffs.
            </p>
          </div>
        )}

        {providerHealth.length > 0 && (
          <ProviderHealthPanel notices={providerHealth} />
        )}

        {status === "disconnected" && (
          <div className="bg-surface-container-low text-on-surface-variant font-label rounded-lg px-4 py-3">
            Disconnected. End this run and create a fresh session before going live again.
          </div>
        )}

        {isReady && streams.length > 0 && (
          <StreamCards
            streams={streams}
            translations={translations}
            isRecording={isRecording}
          />
        )}

        {status === "voice_setup" && !autoSkipVoice && (
          <VoiceSetupCard
            elapsedSec={voiceElapsedSec}
            isRecording={voiceIsRecording}
            minSec={voiceMinSec}
            maxSec={voiceMaxSec}
            onStart={startVoiceRecording}
            onStop={stopVoiceRecording}
            onSkip={skipVoiceSetup}
            sourceLang={
              (SOURCE_LANGS as readonly string[]).includes(sourceLang)
                ? (sourceLang as SourceLang)
                : "en"
            }
          />
        )}

        {status === "cloning" && (
          <div className="flex items-center justify-center gap-3 text-on-surface-variant font-label py-12">
            <Loader2 className="w-5 h-5 animate-spin text-primary" />
            Cloning your voice...
          </div>
        )}

        <div className={cn("flex flex-col items-center gap-2", isReady ? "block" : "hidden")}>
          <div className="h-80 aspect-[9/16] overflow-hidden rounded-xl border-2 border-surface-container-highest bg-black">
            <video
              ref={videoRef}
              autoPlay
              muted
              playsInline
              className="h-full w-full object-contain -scale-x-100"
            />
          </div>
          <span className="text-on-surface-variant text-xs font-label">
            Your camera (mirrored)
          </span>
        </div>

        {isReady && mediaDiagnostics && (
          <MediaDiagnosticsPanel diagnostics={mediaDiagnostics} />
        )}

        {isReady && (
          <AudioRecorder
            isRecording={isRecording}
            analyser={analyser}
            onStart={startRecording}
            onStop={stopRecording}
          />
        )}

        <div ref={listRef} className="space-y-3 max-h-[400px] overflow-y-auto">
          {utterances.map((u) => (
            <div key={u.id} className="bg-surface-container-low rounded-lg px-4 py-3">
              <span className="text-[10px] font-label font-bold uppercase tracking-widest text-primary bg-primary/10 px-2 py-0.5 rounded">
                {session?.source_lang?.toUpperCase() ?? "EN"}
              </span>
              <p className="text-on-surface text-sm mt-1.5">{u.transcript}</p>
            </div>
          ))}

          {liveTranscript && (
            <div className="bg-surface-container-low/50 rounded-lg px-4 py-3 border border-primary/20">
              <span className="text-[10px] font-label font-bold uppercase tracking-widest text-primary flex items-center gap-1.5">
                <span className="w-1.5 h-1.5 bg-primary rounded-full animate-pulse" />
                Live
              </span>
              <p className="text-on-surface text-sm mt-1.5">{liveTranscript}</p>
            </div>
          )}
        </div>

        <LatencyDashboard timings={timings} />
      </main>
    </div>
  );
}


function ProviderHealthPanel({
  notices,
}: {
  notices: ProviderHealthNotice[];
}) {
  return (
    <div className="bg-surface-container-low rounded-xl px-4 py-3 font-label text-xs space-y-2">
      <p className="text-on-surface font-bold">Provider health</p>
      <div className="space-y-1.5">
        {notices.map((notice) => (
          <div
            key={`${notice.provider}:${notice.targetLang ?? ""}:${notice.reason}`}
            className="flex flex-wrap items-center gap-x-2 gap-y-1 text-on-surface-variant"
          >
            <span className="font-bold text-on-surface uppercase">
              {notice.provider}
            </span>
            {notice.targetLang && (
              <span className="rounded bg-surface-container-highest px-1.5 py-0.5 uppercase">
                {notice.targetLang}
              </span>
            )}
            <span
              className={cn(
                "rounded px-1.5 py-0.5 font-bold uppercase",
                notice.billable
                  ? "bg-warning/10 text-warning"
                  : "bg-error-container/30 text-error",
              )}
            >
              {notice.billable ? "billable" : "unbillable"}
            </span>
            <span>{notice.message}</span>
          </div>
        ))}
      </div>
    </div>
  );
}

function MediaDiagnosticsPanel({
  diagnostics,
}: {
  diagnostics: MediaDiagnostics;
}) {
  const src = diagnostics.source;
  const out = diagnostics.outbound;
  return (
    <div className="grid grid-cols-1 sm:grid-cols-2 gap-3 text-xs font-label">
      <div className="bg-surface-container-low rounded-xl px-4 py-3">
        <p className="text-on-surface font-bold mb-1">Browser capture</p>
        <p className="text-on-surface-variant">
          {src?.width ?? "?"}×{src?.height ?? "?"} @ {src?.frameRate ?? "?"}fps
        </p>
      </div>
      <div className="bg-surface-container-low rounded-xl px-4 py-3">
        <p className="text-on-surface font-bold mb-1">WebRTC outbound</p>
        <p className="text-on-surface-variant">
          {out?.frameWidth ?? "?"}×{out?.frameHeight ?? "?"} @ {out?.framesPerSecond ?? "?"}fps
          {typeof out?.framesSent === "number" ? ` · ${out.framesSent} frames` : ""}
        </p>
        {out?.qualityLimitationReason !== undefined && out.qualityLimitationReason !== "none" && (
          <p className="text-warning mt-1">Limited by {String(out.qualityLimitationReason)}</p>
        )}
      </div>
    </div>
  );
}

function displayUrl(url: string): string {
  return url
    .replace("rtmp://rtmp:", "rtmp://localhost:")
    .replace("rtmp://server:", "rtmp://localhost:");
}

function streamPath(rtmpUrl: string): string {
  return rtmpUrl.replace(/^rtmps?:\/\/[^/]+/, "");
}

function getPlatformLabel(platformId: string): string {
  return api.PLATFORMS.find((p) => p.id === platformId)?.label ?? platformId;
}

interface StreamCardsProps {
  streams: api.StreamInfo[];
  translations: Record<string, { id: number; text: string }>;
  isRecording: boolean;
}

function StreamCards({ streams, translations, isRecording }: StreamCardsProps) {
  return (
    <section>
      <div className="flex items-center gap-3 mb-3">
        <h3 className="font-headline font-bold text-lg text-on-surface">Live Streams</h3>
        <span className="text-[10px] font-label font-bold uppercase tracking-widest text-on-surface-variant bg-surface-container-highest px-2 py-0.5 rounded">
          {streams.length} platform{streams.length !== 1 ? "s" : ""}
        </span>
      </div>
      <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-3">
        {streams.map((s) => {
          // Prefer the server-provided watch_url (present for auto-created
          // YouTube broadcasts after this PR). Fall back to reconstructing it
          // from broadcast_id for rows that were created pre-deploy.
          const watchUrl =
            s.watch_url ??
            (s.broadcast_id ? `https://www.youtube.com/watch?v=${s.broadcast_id}` : null);
          return (
            <div key={s.id} className="bg-surface-container-low rounded-xl p-4 space-y-2">
              <div className="flex items-center gap-2">
                <span className="text-[10px] font-label font-bold uppercase tracking-widest text-primary bg-primary/10 px-2 py-0.5 rounded">
                  {s.lang?.toUpperCase()}
                </span>
                <span className="text-on-surface font-label text-sm">
                  {getPlatformLabel(s.platform ?? "custom")}
                </span>
                <span
                  className={cn(
                    "ml-auto text-[10px] font-label font-bold uppercase tracking-widest px-2 py-0.5 rounded",
                    s.error
                      ? "text-error bg-error-container/30"
                      : isRecording
                        ? "text-success bg-success/10"
                        : "text-on-surface-variant bg-surface-container-highest",
                  )}
                >
                  {s.error ? "ERR" : isRecording ? "LIVE" : "READY"}
                </span>
              </div>

              {s.error && <p className="text-error text-xs font-label">{s.error}</p>}

              {translations[s.lang ?? ""] && (
                <p className="text-on-surface text-sm font-label leading-snug border-l-2 border-primary/40 pl-2">
                  {translations[s.lang ?? ""].text}
                </p>
              )}

              {s.rtmp_url && (
                <code className="text-on-surface-variant text-[10px] font-mono block truncate">
                  {displayUrl(s.rtmp_url)}
                </code>
              )}

              {s.platform === "local-test" && s.rtmp_url && (
                <div className="flex flex-col gap-0.5">
                  <a
                    href={`http://localhost:8889${streamPath(s.rtmp_url)}/`}
                    target="_blank"
                    rel="noopener noreferrer"
                    className="flex items-center gap-1 text-primary text-[10px] font-label hover:underline"
                  >
                    <ExternalLink className="w-2.5 h-2.5" />
                    WebRTC
                  </a>
                  <a
                    href={`http://localhost:8888${streamPath(s.rtmp_url)}/`}
                    target="_blank"
                    rel="noopener noreferrer"
                    className="flex items-center gap-1 text-primary text-[10px] font-label hover:underline"
                  >
                    <ExternalLink className="w-2.5 h-2.5" />
                    HLS
                  </a>
                </div>
              )}

              {watchUrl && (
                <WatchUrlRow url={watchUrl} />
              )}
            </div>
          );
        })}
      </div>
    </section>
  );
}

function WatchUrlRow({ url }: { url: string }) {
  const [copied, setCopied] = useState(false);

  const handleCopy = async () => {
    try {
      await navigator.clipboard.writeText(url);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 2000);
    } catch {
      // Clipboard API may be blocked (iframe, insecure context). Fall back
      // to a no-op — the user can still click the link and copy it manually.
    }
  };

  return (
    <div className="flex items-center gap-2">
      <a
        href={url}
        target="_blank"
        rel="noopener noreferrer"
        className="flex items-center gap-1 text-primary text-[10px] font-label hover:underline truncate"
      >
        <ExternalLink className="w-2.5 h-2.5 shrink-0" />
        Watch link
      </a>
      <button
        type="button"
        onClick={handleCopy}
        className="ml-auto flex items-center gap-1 text-on-surface-variant hover:text-on-surface text-[10px] font-label px-1.5 py-0.5 rounded hover:bg-surface-container-high transition-colors"
        title="Copy watch URL"
      >
        {copied ? (
          <>
            <Check className="w-2.5 h-2.5" />
            Copied
          </>
        ) : (
          <>
            <Copy className="w-2.5 h-2.5" />
            Copy
          </>
        )}
      </button>
    </div>
  );
}
