import { useEffect, useRef, useState, useCallback } from "react";
import { useNavigate, useSearchParams } from "react-router-dom";
import { useHostRoom } from "../hooks/useHostRoom";
import { AudioRecorder } from "../components/AudioRecorder";
import { LatencyDashboard } from "../components/LatencyDashboard";
import {
  ArrowLeft,
  Loader2,
  Mic,
  SkipForward,
  ExternalLink,
} from "lucide-react";
import { cn } from "../lib/cn";
import * as api from "../lib/api";

export default function HostPage() {
  const navigate = useNavigate();
  const [searchParams] = useSearchParams();
  const sessionId = searchParams.get("sessionId") ?? undefined;
  const sourceLang = searchParams.get("sourceLang") ?? "en";
  const {
    status,
    liveTranscript,
    utterances,
    analyser,
    error,
    timings,
    videoRef,
    createRoom,
    startRecording,
    stopRecording,
    closeRoom,
    startVoiceRecording,
    skipVoiceSetup,
  } = useHostRoom();

  const listRef = useRef<HTMLDivElement>(null);
  const createdRef = useRef(false);

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
    loadSession();
  }, [loadSession]);

  useEffect(() => {
    if (createdRef.current) return;
    createdRef.current = true;
    createRoom({ sessionId, sourceLang });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    if (listRef.current) {
      listRef.current.scrollTop = listRef.current.scrollHeight;
    }
  }, [utterances, liveTranscript]);

  const handleBack = () => {
    closeRoom();
    navigate(sessionId ? `/session/${sessionId}` : "/dashboard");
  };

  // Voice recording timer
  const [voiceTimer, setVoiceTimer] = useState(0);
  const [isVoiceRecording, setIsVoiceRecording] = useState(false);
  const voiceTimerRef = useRef<ReturnType<typeof setInterval> | null>(null);

  const handleStartVoice = () => {
    setVoiceTimer(30);
    setIsVoiceRecording(true);
    startVoiceRecording();
    voiceTimerRef.current = setInterval(() => {
      setVoiceTimer((t) => {
        if (t <= 1) {
          clearInterval(voiceTimerRef.current!);
          setIsVoiceRecording(false);
          return 0;
        }
        return t - 1;
      });
    }, 1000);
  };

  const isReady = status === "ready" || status === "recording";
  const isRecording = status === "recording";

  const getPlatformLabel = (platformId: string) =>
    api.PLATFORMS.find((p) => p.id === platformId)?.label ?? platformId;

  const displayUrl = (url: string) =>
    url
      .replace("rtmp://rtmp:", "rtmp://localhost:")
      .replace("rtmp://server:", "rtmp://localhost:");

  const streamPath = (rtmpUrl: string) =>
    rtmpUrl.replace(/^rtmps?:\/\/[^/]+/, "");

  return (
    <div className="min-h-screen bg-background">
      {/* Header */}
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
            {session ? session.title : "Quick Room"}
          </p>
        </div>
      </header>

      <main className="max-w-5xl mx-auto px-8 pt-24 pb-16 space-y-8">
        {/* Error */}
        {error && (
          <div className="bg-error-container/20 text-error px-4 py-2.5 rounded-lg font-label text-sm">
            {error}
          </div>
        )}

        {/* Status messages */}
        {status === "creating" && (
          <div className="flex items-center gap-3 text-on-surface-variant font-label">
            <Loader2 className="w-4 h-4 animate-spin" />
            Creating room...
          </div>
        )}

        {status === "disconnected" && (
          <div className="text-on-surface-variant font-label">
            Disconnected.
          </div>
        )}

        {/* Stream Status Cards */}
        {isReady && streams.length > 0 && (
          <section>
            <div className="flex items-center gap-3 mb-3">
              <h3 className="font-headline font-bold text-lg text-on-surface">
                Live Streams
              </h3>
              <span className="text-[10px] font-label font-bold uppercase tracking-widest text-on-surface-variant bg-surface-container-highest px-2 py-0.5 rounded">
                {streams.length} platform
                {streams.length !== 1 ? "s" : ""}
              </span>
            </div>
            <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-3">
              {streams.map((s) => (
                <div
                  key={s.id}
                  className="bg-surface-container-low rounded-xl p-4 space-y-2"
                >
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
                            : "text-on-surface-variant bg-surface-container-highest"
                      )}
                    >
                      {s.error ? "ERR" : isRecording ? "LIVE" : "READY"}
                    </span>
                  </div>

                  {s.error && (
                    <p className="text-error text-xs font-label">{s.error}</p>
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
        )}

        {/* Voice Setup Phase */}
        {status === "voice_setup" && (
          <section className="bg-surface-container-low rounded-xl p-8 max-w-lg mx-auto space-y-6">
            <div>
              <h3 className="font-headline font-bold text-2xl text-on-surface mb-2">
                Voice Setup
              </h3>
              <p className="text-on-surface-variant text-sm leading-relaxed">
                Record a 30-second voice sample to clone your voice. Read the
                text below naturally at your normal pace.
              </p>
            </div>

            {!isVoiceRecording && voiceTimer === 0 && (
              <button
                className="monolith-gradient text-white w-full py-3 rounded-xl font-headline font-bold hover:scale-[0.98] transition-all flex items-center justify-center gap-2"
                onClick={handleStartVoice}
              >
                <Mic className="w-5 h-5" />
                Record Voice Sample
              </button>
            )}

            {isVoiceRecording && (
              <>
                <div className="flex items-center gap-3 text-error font-label">
                  <span className="w-2 h-2 bg-error rounded-full animate-pulse" />
                  Recording... {voiceTimer}s
                </div>
                <div className="bg-surface-container-highest rounded-lg p-4 text-on-surface-variant text-sm leading-relaxed font-body italic">
                  Welcome to today's live stream! I'm really excited to show you
                  some amazing products that I've been using lately. These items
                  have completely changed my daily routine, and I think you're
                  going to love them too. The quality is outstanding, and the
                  price is incredibly reasonable for what you get. I've tried
                  many similar products before, but nothing comes close to this.
                  If you have any questions, feel free to drop them in the chat
                  and I'll answer them right away. Let's get started!
                </div>
              </>
            )}

            <button
              className="w-full bg-surface-container-high hover:bg-surface-bright text-on-surface-variant py-2.5 rounded-lg font-label text-sm transition-colors flex items-center justify-center gap-2"
              onClick={skipVoiceSetup}
            >
              <SkipForward className="w-4 h-4" />
              Skip (use default voice)
            </button>
          </section>
        )}

        {status === "cloning" && (
          <div className="flex items-center justify-center gap-3 text-on-surface-variant font-label py-12">
            <Loader2 className="w-5 h-5 animate-spin text-primary" />
            Cloning your voice...
          </div>
        )}

        {/* Webcam preview */}
        <div
          className={cn(
            "flex flex-col items-center gap-2",
            isReady ? "block" : "hidden"
          )}
        >
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

        {/* Audio recorder */}
        {isReady && (
          <AudioRecorder
            isRecording={isRecording}
            analyser={analyser}
            onStart={startRecording}
            onStop={stopRecording}
          />
        )}

        {/* Utterances */}
        <div ref={listRef} className="space-y-3 max-h-[400px] overflow-y-auto">
          {utterances.map((u) => (
            <div
              key={u.id}
              className="bg-surface-container-low rounded-lg px-4 py-3"
            >
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
              <p className="text-on-surface text-sm mt-1.5">
                {liveTranscript}
              </p>
            </div>
          )}
        </div>

        {/* Latency dashboard */}
        <LatencyDashboard timings={timings} />
      </main>
    </div>
  );
}
