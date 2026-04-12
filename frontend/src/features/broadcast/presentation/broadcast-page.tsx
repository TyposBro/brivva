import { useRef, useMemo, useEffect } from "react";
import { PageLayout } from "../../../shared/ui-kit/page-layout";
import { useBroadcastConfig } from "./hooks/use-broadcast-config";
import { useWebcam } from "./hooks/use-webcam";
import { useVoiceClone } from "./hooks/use-voice-clone";
import { useBroadcastSocket } from "./hooks/use-broadcast-socket";
import { useMediaDevices } from "./hooks/use-media-devices";
import { useBroadcastHandlers } from "./hooks/use-broadcast-handlers";
import { usePipelineHealth } from "./hooks/use-pipeline-health";
import { useDriftWarning } from "./hooks/use-drift-warning";
import { TierSelector } from "./components/tier-selector";
import { LanguageConfig } from "./components/language-config";
import { RtmpDestinations } from "./components/rtmp-destinations";
import { BroadcastSettings } from "./components/broadcast-settings";
import { VoiceCloning } from "./components/voice-cloning";
import { LiveTranscript } from "./components/live-transcript";
import { BroadcastControls } from "./components/broadcast-controls";
import { ErrorBanners } from "./components/error-banners";
import { InfoFooter } from "./components/info-footer";
import { PipelineHealthBadge } from "./components/pipeline-health-badge";
import { HealthDashboard } from "./components/health-dashboard";
import { DriftWarningBanner } from "./components/drift-warning-banner";
// PRAGMATIC: cross-feature UI integration — dubbing panel rendered within broadcast page
import { DubbingPanel } from "../../dubbing/presentation/components/dubbing-panel";

export default function BroadcastPage() {
  const { config, update } = useBroadcastConfig();
  const devices = useMediaDevices();
  const videoRef = useRef<HTMLVideoElement>(null);
  const { startWebcam, stopWebcam } = useWebcam(videoRef);
  const { health, handleHealthMessage, resetHealth } = usePipelineHealth();
  const { showDriftWarning, checkDrift, dismissDriftWarning } =
    useDriftWarning(config.broadcastDelay);

  useEffect(() => { checkDrift(health); }, [health, checkDrift]);

  const socketParams = useMemo(() => ({
    sourceLang: config.sourceLang,
    targetLangs: config.targetLangs,
    tier: config.tier,
    ttsModel: config.ttsModel,
    ttsProvider: config.ttsProvider,
    audioDeviceId: config.audioDeviceId,
    rtmpUrls: config.rtmpUrls,
    broadcastDelay: config.broadcastDelay,
    onStartWebcam: (ws: WebSocket) => startWebcam(ws, config.videoDeviceId),
    onStopWebcam: stopWebcam,
    onHealthMessage: handleHealthMessage,
  }), [config, startWebcam, stopWebcam, handleHealthMessage]);

  const { isLive, sessionId, interim, transcripts, errors, pipelineWarnings, wsRef, start, stop, addError, dismissError } =
    useBroadcastSocket(socketParams);

  const { phase, elapsedSec, isMinReached, voiceReady, setVoiceReady, cloneVoice, stopCloning } = useVoiceClone(addError, config.ttsProvider);

  const {
    handleSourceChange, handleTargetToggle, handleRtmpChange,
    handleRtmpRestart, handleStart, hasRtmpStreams, canStart,
  } = useBroadcastHandlers({ config, update, wsRef, start, setVoiceReady });

  const showVoiceCloning = !isLive && config.tier >= 2;
  const videoClassName = isLive && hasRtmpStreams
    ? "w-full max-h-48 object-cover rounded-xl bg-black"
    : "hidden";

  useEffect(() => {
    if (!isLive) resetHealth();
  }, [isLive, resetHealth]);

  return (
    <PageLayout
      maxWidth="3xl"
      spaceY={6}
      right={
        sessionId
          ? <span className="text-xs font-mono text-outline">Session: {sessionId}</span>
          : undefined
      }
    >
      <ErrorBanners errors={errors} onDismiss={dismissError} />
      {showDriftWarning && <DriftWarningBanner onDismiss={dismissDriftWarning} />}

      <TierSelector
        tier={config.tier}
        isLive={isLive}
        onTierChange={(t) => update("tier", t)}
      />
      <LanguageConfig
        sourceLang={config.sourceLang}
        targetLangs={config.targetLangs}
        isLive={isLive}
        onSourceChange={handleSourceChange}
        onTargetToggle={handleTargetToggle}
      />
      <RtmpDestinations
        sourceLang={config.sourceLang}
        targetLangs={config.targetLangs}
        rtmpUrls={config.rtmpUrls}
        isLive={isLive}
        onRtmpChange={handleRtmpChange}
      />
      <BroadcastSettings
        config={config}
        devices={devices}
        isLive={isLive}
        onConfigChange={update}
      />

      {showVoiceCloning && (
        <VoiceCloning
          phase={phase}
          elapsedSec={elapsedSec}
          isMinReached={isMinReached}
          voiceReady={voiceReady}
          onClone={cloneVoice}
          onStop={stopCloning}
        />
      )}

      <video ref={videoRef} className={videoClassName} muted playsInline />

      <BroadcastControls
        isLive={isLive}
        canStart={canStart}
        hasRtmpStreams={hasRtmpStreams}
        voiceReady={voiceReady}
        onStart={handleStart}
        onStop={stop}
        onRtmpRestart={handleRtmpRestart}
      />

      {!isLive && sessionId && config.tier >= 4 && (
        <DubbingPanel
          sessionId={sessionId}
          targetLangs={config.targetLangs}
          sourceLang={config.sourceLang}
        />
      )}

      {isLive && <PipelineHealthBadge warnings={pipelineWarnings} />}
      {isLive && health && <HealthDashboard health={health} />}
      {isLive && <LiveTranscript transcripts={transcripts} interim={interim} />}
      <InfoFooter tier={config.tier} hasRtmpStreams={hasRtmpStreams} />
    </PageLayout>
  );
}
