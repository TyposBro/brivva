import { useRef, useMemo } from "react";
import { PageLayout } from "../../shared/components/PageLayout";
import { useBroadcastConfig } from "./hooks/useBroadcastConfig";
import { useWebcam } from "./hooks/useWebcam";
import { useVoiceClone } from "./hooks/useVoiceClone";
import { useBroadcastSocket } from "./hooks/useBroadcastSocket";
import { useMediaDevices } from "./hooks/useMediaDevices";
import { useBroadcastHandlers } from "./hooks/useBroadcastHandlers";
import { TierSelector } from "./components/TierSelector";
import { LanguageConfig } from "./components/LanguageConfig";
import { RtmpDestinations } from "./components/RtmpDestinations";
import { BroadcastSettings } from "./components/BroadcastSettings";
import { VoiceCloning } from "./components/VoiceCloning";
import { LiveTranscript } from "./components/LiveTranscript";
import { BroadcastControls } from "./components/BroadcastControls";
import { ErrorBanners } from "./components/ErrorBanners";
import { InfoFooter } from "./components/InfoFooter";

export default function BroadcastPage() {
  const { config, update } = useBroadcastConfig();
  const devices = useMediaDevices();
  const videoRef = useRef<HTMLVideoElement>(null);
  const { startWebcam, stopWebcam } = useWebcam(videoRef);

  const socketParams = useMemo(() => ({
    sourceLang: config.sourceLang,
    targetLangs: config.targetLangs,
    tier: config.tier,
    ttsModel: config.ttsModel,
    audioDeviceId: config.audioDeviceId,
    rtmpUrls: config.rtmpUrls,
    broadcastDelay: config.broadcastDelay,
    onStartWebcam: (ws: WebSocket) => startWebcam(ws, config.videoDeviceId),
    onStopWebcam: stopWebcam,
  }), [config, startWebcam, stopWebcam]);

  const { isLive, sessionId, interim, transcripts, errors, wsRef, start, stop, addError, dismissError } =
    useBroadcastSocket(socketParams);

  const { isCloning, cloneProgress, voiceReady, setVoiceReady, cloneVoice } = useVoiceClone(addError);

  const {
    handleSourceChange, handleTargetToggle, handleRtmpChange,
    handleRtmpRestart, handleStart, hasRtmpStreams, canStart,
  } = useBroadcastHandlers({ config, update, wsRef, start, setVoiceReady });

  const showVoiceCloning = !isLive && config.tier >= 2;
  const videoClassName = isLive && hasRtmpStreams
    ? "w-full max-h-48 object-cover rounded-xl bg-black"
    : "hidden";

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
          isCloning={isCloning}
          cloneProgress={cloneProgress}
          voiceReady={voiceReady}
          onClone={cloneVoice}
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

      {isLive && <LiveTranscript transcripts={transcripts} interim={interim} />}
      <InfoFooter tier={config.tier} hasRtmpStreams={hasRtmpStreams} />
    </PageLayout>
  );
}
