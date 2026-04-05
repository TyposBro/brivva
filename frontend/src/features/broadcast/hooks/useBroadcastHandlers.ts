import { useCallback } from "react";
import type { BroadcastConfig } from "./useBroadcastConfig";

type Deps = {
  config: BroadcastConfig;
  update: <K extends keyof BroadcastConfig>(key: K, value: BroadcastConfig[K]) => void;
  wsRef: React.RefObject<WebSocket | null>;
  start: () => Promise<void>;
  setVoiceReady: (v: boolean) => void;
};

export function useBroadcastHandlers({ config, update, wsRef, start, setVoiceReady }: Deps) {
  const handleSourceChange = useCallback((code: string) => {
    update("sourceLang", code);
    update("targetLangs", config.targetLangs.filter((t) => t !== code));
  }, [config.targetLangs, update]);

  const handleTargetToggle = useCallback((code: string) => {
    const prev = config.targetLangs;
    const next = prev.includes(code) ? prev.filter((l) => l !== code) : [...prev, code];
    update("targetLangs", next);
  }, [config.targetLangs, update]);

  const handleRtmpChange = useCallback((lang: string, url: string) => {
    update("rtmpUrls", { ...config.rtmpUrls, [lang]: url });
  }, [config.rtmpUrls, update]);

  const handleRtmpRestart = useCallback(() => {
    wsRef.current?.send(JSON.stringify({ type: "rtmp:restart" }));
  }, [wsRef]);

  const handleStart = useCallback(async () => {
    await start();
    setVoiceReady(false);
  }, [start, setVoiceReady]);

  const hasRtmpStreams = Object.values(config.rtmpUrls).some((url) => url.trim());
  const canStart = config.targetLangs.filter((l) => l !== config.sourceLang).length > 0;

  return {
    handleSourceChange,
    handleTargetToggle,
    handleRtmpChange,
    handleRtmpRestart,
    handleStart,
    hasRtmpStreams,
    canStart,
  };
}
