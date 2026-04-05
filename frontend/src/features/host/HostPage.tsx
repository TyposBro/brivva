import { useEffect, useRef } from "react";
import { useNavigate, useSearchParams } from "react-router-dom";
import { useHostRoom } from "../../hooks/useHostRoom";
import { AudioRecorder } from "../../components/audio-recorder";
import { LatencyDashboard } from "../../components/latency-dashboard";
import { PageLayout } from "../../shared/components/PageLayout";
import { BackButton } from "../../shared/components/BackButton";
import { useVoiceTimer } from "./hooks/useVoiceTimer";
import { useHostSession } from "./hooks/useHostSession";
import { CreatingStatus } from "./components/CreatingStatus";
import { DisconnectedStatus } from "./components/DisconnectedStatus";
import { CloningStatus } from "./components/CloningStatus";
import { CameraPreview } from "./components/CameraPreview";
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
    <PageLayout
      left={<BackButton onClick={handleBack} />}
      right={
        <p className="text-on-surface-variant font-label text-sm truncate max-w-[200px]">
          {session ? session.title : "Quick Room"}
        </p>
      }
    >
      {error && (
        <div className="bg-error-container/20 text-error px-4 py-2.5 rounded-lg font-label text-sm">
          {error}
        </div>
      )}

      {status === "creating" && <CreatingStatus />}
      {status === "disconnected" && <DisconnectedStatus />}
      {isReady && <StreamCards streams={streams} isRecording={isRecording} />}
      {status === "voice_setup" && (
        <VoiceSetup
          isVoiceRecording={isVoiceRecording}
          voiceTimer={voiceTimer}
          onStartVoice={handleStartVoice}
          onSkip={skipVoiceSetup}
        />
      )}
      {status === "cloning" && <CloningStatus />}
      <CameraPreview videoRef={videoRef} visible={isReady} />
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
    </PageLayout>
  );
}
