import { useEffect, useRef } from "react";
import { useNavigate, useSearchParams } from "react-router-dom";
import { ArrowLeft, Loader2 } from "lucide-react";
import { cn } from "../../lib/cn";
import { useHostRoom } from "../../hooks/useHostRoom";
import { AudioRecorder } from "../../components/AudioRecorder";
import { LatencyDashboard } from "../../components/LatencyDashboard";
import { useVoiceTimer } from "./hooks/useVoiceTimer";
import { useHostSession } from "./hooks/useHostSession";
import { StreamCards } from "./components/StreamCards";
import { VoiceSetup } from "./components/VoiceSetup";
import { UtteranceList } from "./components/UtteranceList";

export default function HostPage() {
  const navigate = useNavigate();
  const [searchParams] = useSearchParams();
  const sessionId = searchParams.get("sessionId") ?? undefined;
  const sourceLang = searchParams.get("sourceLang") ?? "en";

  const {
    status, liveTranscript, utterances, analyser, error, timings, videoRef,
    createRoom, startRecording, stopRecording, closeRoom,
    startVoiceRecording, skipVoiceSetup,
  } = useHostRoom();

  const { session, streams, loadSession } = useHostSession();
  const { voiceTimer, isVoiceRecording, handleStartVoice } =
    useVoiceTimer(startVoiceRecording);

  const createdRef = useRef(false);

  useEffect(() => { loadSession(sessionId); }, [loadSession, sessionId]);

  useEffect(() => {
    if (createdRef.current) return;
    createdRef.current = true;
    createRoom({ sessionId, sourceLang });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const handleBack = () => {
    closeRoom();
    navigate(sessionId ? `/session/${sessionId}` : "/dashboard");
  };

  const isReady = status === "ready" || status === "recording";
  const isRecording = status === "recording";
  const sourceLangLabel = session?.source_lang?.toUpperCase() ?? "EN";

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
            {session ? session.title : "Quick Room"}
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
            Creating room...
          </div>
        )}

        {status === "disconnected" && (
          <div className="text-on-surface-variant font-label">Disconnected.</div>
        )}

        {isReady && <StreamCards streams={streams} isRecording={isRecording} />}

        {status === "voice_setup" && (
          <VoiceSetup
            isVoiceRecording={isVoiceRecording}
            voiceTimer={voiceTimer}
            onStartVoice={handleStartVoice}
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

        <UtteranceList
          utterances={utterances}
          liveTranscript={liveTranscript}
          sourceLangLabel={sourceLangLabel}
        />

        <LatencyDashboard timings={timings} />
      </main>
    </div>
  );
}
