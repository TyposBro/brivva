import { useRef, useCallback, useEffect, useState, useMemo } from "react";
import { useBroadcastConfig } from "./hooks/useBroadcastConfig";
import { useWebcam } from "./hooks/useWebcam";
import { useVoiceClone } from "./hooks/useVoiceClone";
import { useBroadcastSocket } from "./hooks/useBroadcastSocket";
import { TierSelector } from "./components/TierSelector";
import { LanguageConfig } from "./components/LanguageConfig";
import { RtmpDestinations } from "./components/RtmpDestinations";
import { BroadcastSettings } from "./components/BroadcastSettings";
import { VoiceCloning } from "./components/VoiceCloning";
import { LiveTranscript } from "./components/LiveTranscript";
import { BroadcastControls } from "./components/BroadcastControls";

export default function BroadcastPage() {
  const { config, update } = useBroadcastConfig();
  const [devices, setDevices] = useState<MediaDeviceInfo[]>([]);
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

  const { isLive, sessionId, interim, transcripts, errors, wsRef, start, stop, addError, dismissError } = useBroadcastSocket(socketParams);

  const { isCloning, cloneProgress, voiceReady, setVoiceReady, cloneVoice } = useVoiceClone(addError);

  // Enumerate media devices on mount
  useEffect(() => {
    (async () => {
      try {
        const stream = await navigator.mediaDevices.getUserMedia({ audio: true, video: true });
        stream.getTracks().forEach((t) => t.stop());
      } catch {}
      setDevices(await navigator.mediaDevices.enumerateDevices());
    })();
  }, []);

  const handleSourceChange = useCallback((code: string) => {
    update("sourceLang", code);
    update("targetLangs", config.targetLangs.filter((t) => t !== code));
  }, [config.targetLangs, update]);

  const handleTargetToggle = useCallback((code: string) => {
    const prev = config.targetLangs;
    update("targetLangs", prev.includes(code) ? prev.filter((l) => l !== code) : [...prev, code]);
  }, [config.targetLangs, update]);

  const handleRtmpChange = useCallback((lang: string, url: string) => {
    update("rtmpUrls", { ...config.rtmpUrls, [lang]: url });
  }, [config.rtmpUrls, update]);

  const handleRtmpRestart = useCallback(() => {
    wsRef.current?.send(JSON.stringify({ type: "rtmp:restart" }));
  }, [wsRef]);

  // Clear voiceReady when going live (original behavior)
  const handleStart = useCallback(async () => {
    await start();
    setVoiceReady(false);
  }, [start, setVoiceReady]);

  const hasRtmpStreams = Object.values(config.rtmpUrls).some((url) => url.trim());
  const canStart = config.targetLangs.filter((l) => l !== config.sourceLang).length > 0;

  return (
    <div className="min-h-screen bg-background text-on-surface p-6">
      <div className="max-w-3xl mx-auto space-y-6">
        <Header sessionId={sessionId} />
        <ErrorBanners errors={errors} onDismiss={dismissError} />
        <TierSelector tier={config.tier} isLive={isLive} onTierChange={(t) => update("tier", t)} />
        <LanguageConfig sourceLang={config.sourceLang} targetLangs={config.targetLangs} isLive={isLive} onSourceChange={handleSourceChange} onTargetToggle={handleTargetToggle} />
        <RtmpDestinations sourceLang={config.sourceLang} targetLangs={config.targetLangs} rtmpUrls={config.rtmpUrls} isLive={isLive} onRtmpChange={handleRtmpChange} />
        <BroadcastSettings config={config} devices={devices} isLive={isLive} onConfigChange={update} />
        {!isLive && config.tier >= 2 && <VoiceCloning isCloning={isCloning} cloneProgress={cloneProgress} voiceReady={voiceReady} onClone={cloneVoice} />}
        <video ref={videoRef} className={isLive && hasRtmpStreams ? "w-full max-h-48 object-cover rounded-xl bg-black" : "hidden"} muted playsInline />
        <BroadcastControls isLive={isLive} canStart={canStart} hasRtmpStreams={hasRtmpStreams} voiceReady={voiceReady} onStart={handleStart} onStop={stop} onRtmpRestart={handleRtmpRestart} />
        {isLive && <LiveTranscript transcripts={transcripts} interim={interim} />}
        <InfoFooter tier={config.tier} hasRtmpStreams={hasRtmpStreams} />
      </div>
    </div>
  );
}

function Header({ sessionId }: { sessionId: string | null }) {
  return (
    <div className="flex items-center justify-between">
      <h1 className="font-headline text-2xl font-bold text-primary">Brivva</h1>
      {sessionId && <span className="text-xs font-mono text-outline">Session: {sessionId}</span>}
    </div>
  );
}

function ErrorBanners({ errors, onDismiss }: { errors: string[]; onDismiss: (i: number) => void }) {
  return (
    <>
      {errors.map((err, i) => (
        <div key={i} className="flex items-center justify-between bg-error-container text-on-error-container rounded-lg px-4 py-2 text-sm">
          <span>{err}</span>
          <button onClick={() => onDismiss(i)} className="ml-4 text-on-error-container/60 hover:text-on-error-container text-lg leading-none">x</button>
        </div>
      ))}
    </>
  );
}

function InfoFooter({ tier, hasRtmpStreams }: { tier: number; hasRtmpStreams: boolean }) {
  return (
    <div className="text-xs text-outline space-y-1">
      <p>Option {tier}: {tier === 1 ? "Subtitles only — original audio passes through" : "Translated voice + subtitles — each language gets AI-generated voice audio"}</p>
      <p>{hasRtmpStreams ? "Webcam video + translated audio muxed via FFmpeg and pushed to RTMP destinations." : "Audio-only mode. Add RTMP destinations above to enable video streaming."}</p>
    </div>
  );
}
