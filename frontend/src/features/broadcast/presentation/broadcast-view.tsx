import { useEffect, useRef, useState, useCallback } from "react";
import { useNavigate } from "react-router-dom";
import { ArrowLeft, ExternalLink, Loader2 } from "lucide-react";
import { useHostSession } from "./use-host-session";
import { AudioRecorder } from "./audio-recorder";
import { LatencyDashboard } from "./latency-dashboard";
import { VoiceSetupCard } from "./voice-setup-card";
import { cn } from "../../../core/cn";
import * as api from "../data/api-client";

export interface BroadcastViewProps {
  sessionId?: string;
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
    if (!sessionId) return;
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
    navigate(sessionId ? `/session/${sessionId}` : "/dashboard");
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
            {session ? session.title : "Quick Session"}
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

        {status === "disconnected" && (
          <div className="text-on-surface-variant font-label">Disconnected.</div>
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
          />
        )}

        {status === "cloning" && (
          <div className="flex items-center justify-center gap-3 text-on-surface-variant font-label py-12">
            <Loader2 className="w-5 h-5 animate-spin text-primary" />
            Cloning your voice...
          </div>
        )}

        <div className={cn("flex flex-col items-center gap-2", isReady ? "block" : "hidden")}>
          <video
            ref={videoRef}
            autoPlay
            muted
            playsInline
            className="w-64 h-64 object-cover -scale-x-100 rounded-xl border-2 border-surface-container-highest"
          />
          <span className="text-on-surface-variant text-xs font-label">
            Your camera (mirrored)
          </span>
        </div>

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
        {streams.map((s) => (
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

            {s.broadcast_id && (
              <a
                href={`https://youtube.com/watch?v=${s.broadcast_id}`}
                target="_blank"
                rel="noopener noreferrer"
                className="flex items-center gap-1 text-primary text-[10px] font-label hover:underline"
              >
                <ExternalLink className="w-2.5 h-2.5" />
                YouTube
              </a>
            )}
          </div>
        ))}
      </div>
    </section>
  );
}
