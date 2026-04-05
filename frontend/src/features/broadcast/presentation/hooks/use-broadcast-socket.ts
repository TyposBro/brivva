import { useState, useRef, useCallback } from "react";
import { appConfig } from "../../../../orchestration/config/app-config";
import { AudioPipeline } from "../../../../shared/media/audio-pipeline";
import type { TranscriptEntry, TranslationTier } from "../../domain/broadcast-types";
import { AUDIO_TAG } from "../../data/dtos";

type SocketParams = {
  sourceLang: string;
  targetLangs: string[];
  tier: TranslationTier;
  ttsModel: string;
  audioDeviceId: string;
  rtmpUrls: Record<string, string>;
  broadcastDelay: number;
  onStartWebcam: (ws: WebSocket) => Promise<void>;
  onStopWebcam: () => void;
};

export function useBroadcastSocket(params: SocketParams) {
  const [isLive, setIsLive] = useState(false);
  const [sessionId, setSessionId] = useState<string | null>(null);
  const [interim, setInterim] = useState("");
  const [transcripts, setTranscripts] = useState<TranscriptEntry[]>([]);
  const [errors, setErrors] = useState<string[]>([]);

  const wsRef = useRef<WebSocket | null>(null);
  const audioRef = useRef(new AudioPipeline());

  const addError = useCallback((msg: string) => {
    setErrors((prev) => [...prev, msg]);
  }, []);

  const dispatch = { setInterim, setTranscripts };

  const handleMessage = useCallback((e: MessageEvent) => {
    if (typeof e.data !== "string") return;
    const msg = JSON.parse(e.data);

    switch (msg.type) {
      case "session:created":  setSessionId(msg.id); break;
      case "interim":          setInterim(msg.transcript); break;
      case "final":            handleFinal(msg, dispatch); break;
      case "translation":      handleTranslation(msg, dispatch); break;
      case "chunk_translation": handleChunkTranslation(msg, dispatch); break;
      case "error":            addError(msg.message); break;
    }
  }, [addError]);

  const start = useCallback(async () => {
    const available = filterTargetLanguages(params.sourceLang, params.targetLangs);
    if (available.length === 0) return;

    const ws = await connectWebSocket({ params, targets: available, onMessage: handleMessage, onError: addError });
    if (!ws) return;

    wsRef.current = ws;
    registerCloseHandler(ws, { addError, setIsLive, setSessionId });
    await configureRtmpStreams(ws, params);
    await startAudioCapture(ws, { audio: audioRef.current, deviceId: params.audioDeviceId });

    setIsLive(true);
    setTranscripts([]);
    setInterim("");
  }, [params, handleMessage, addError]);

  const stop = useCallback(() => {
    params.onStopWebcam();
    audioRef.current.stop();
    wsRef.current?.close();
    wsRef.current = null;
    setIsLive(false);
    setSessionId(null);
  }, [params]);

  const dismissError = useCallback((index: number) => {
    setErrors((prev) => prev.filter((_, j) => j !== index));
  }, []);

  return { isLive, sessionId, interim, transcripts, errors, wsRef, start, stop, addError, dismissError };
}

// ── Helpers ──────────────────────────────────────────────

function filterTargetLanguages(sourceLang: string, targetLangs: string[]): string[] {
  return targetLangs.filter((l) => l !== sourceLang);
}

function buildWsUrl(params: SocketParams, targets: string[]): string {
  const base = appConfig.apiBaseUrl.replace(/^http/, "ws");
  return `${base}/ws?sourceLang=${params.sourceLang}&targetLangs=${targets.join(",")}&tier=${params.tier}&ttsModel=${params.ttsModel}`;
}

type ConnectConfig = {
  params: SocketParams;
  targets: string[];
  onMessage: (e: MessageEvent) => void;
  onError: (msg: string) => void;
};

async function connectWebSocket({ params, targets, onMessage, onError }: ConnectConfig): Promise<WebSocket | null> {
  const url = buildWsUrl(params, targets);
  const ws = new WebSocket(url);
  ws.binaryType = "arraybuffer";
  ws.onmessage = onMessage;

  try {
    await waitForOpen(ws);
    return ws;
  } catch {
    onError("Cannot connect \u2014 is the backend running?");
    return null;
  }
}

type CloseCallbacks = {
  addError: (msg: string) => void;
  setIsLive: (v: boolean) => void;
  setSessionId: (v: string | null) => void;
};

function registerCloseHandler(ws: WebSocket, { addError, setIsLive, setSessionId }: CloseCallbacks): void {
  ws.onclose = (ev) => {
    if (ev.code !== 1000) addError(`Connection lost (code ${ev.code}). Restart to reconnect.`);
    setIsLive(false);
    setSessionId(null);
  };
  ws.onerror = () => {};
}

async function configureRtmpStreams(ws: WebSocket, params: SocketParams): Promise<void> {
  const streams = Object.entries(params.rtmpUrls)
    .filter(([, url]) => url.trim())
    .map(([lang, url]) => ({ lang, url: url.trim() }));

  if (streams.length === 0) return;

  await params.onStartWebcam(ws);
  ws.send(JSON.stringify({ type: "rtmp:config", streams, broadcastDelay: params.broadcastDelay }));
}

type AudioCapture = {
  audio: AudioPipeline;
  deviceId: string;
};

async function startAudioCapture(ws: WebSocket, { audio, deviceId }: AudioCapture): Promise<void> {
  await audio.start((buffer) => {
    if (ws.readyState !== WebSocket.OPEN) return;
    const tagged = new Uint8Array(buffer.byteLength + 1);
    tagged[0] = AUDIO_TAG;
    tagged.set(new Uint8Array(buffer), 1);
    ws.send(tagged.buffer);
  }, deviceId || undefined);
}

// ── Message handlers ─────────────────────────────────────

type TranscriptDispatch = {
  setInterim: (s: string) => void;
  setTranscripts: React.Dispatch<React.SetStateAction<TranscriptEntry[]>>;
};

function handleFinal(
  msg: { utteranceId: number; transcript: string },
  { setInterim, setTranscripts }: TranscriptDispatch,
) {
  setInterim("");
  setTranscripts((prev) => [...prev, { id: msg.utteranceId, text: msg.transcript, translations: {} }]);
}

function handleTranslation(
  msg: { utteranceId: number; lang: string; text: string },
  { setTranscripts }: TranscriptDispatch,
) {
  setTranscripts((prev) =>
    prev.map((t) =>
      t.id === msg.utteranceId
        ? { ...t, translations: { ...t.translations, [msg.lang]: msg.text } }
        : t
    )
  );
}

function handleChunkTranslation(
  msg: { utteranceId: number; lang: string; text: string; chunkIndex: number },
  { setTranscripts }: TranscriptDispatch,
) {
  setTranscripts((prev) => {
    const exists = prev.some((t) => t.id === msg.utteranceId);
    const list = exists ? prev : [...prev, { id: msg.utteranceId, text: "...", translations: {} }];
    return list.map((t) => {
      if (t.id !== msg.utteranceId) return t;
      const existing = t.translations[msg.lang] || "";
      const separator = existing && msg.chunkIndex > 0 ? " " : "";
      return { ...t, translations: { ...t.translations, [msg.lang]: existing + separator + msg.text } };
    });
  });
}

function waitForOpen(ws: WebSocket): Promise<void> {
  return new Promise((resolve, reject) => {
    ws.onopen = () => resolve();
    ws.onerror = () => reject();
  });
}
