import { useState, useRef, useCallback, type MutableRefObject } from "react";
import { appConfig } from "../../../../orchestration/config/app-config";
import { AudioPipeline } from "../../../../shared/media/audio-pipeline";
import type { TranscriptEntry, TranslationTier, VoiceMode } from "../../domain/broadcast-types";
import type { PipelineHealthMsg } from "../../data/dtos";
import { AUDIO_TAG } from "../../data/dtos";
import { useDeduplicatedErrors } from "./use-deduplicated-errors";
import { deriveVoiceDefaultLangs, deriveVoiceGenderMap } from "./use-broadcast-config";

type SocketParams = {
  sourceLang: string;
  targetLangs: string[];
  tier: TranslationTier;
  ttsModel: string;
  ttsProvider: string;
  voiceConfig: Record<string, VoiceMode>;
  audioDeviceId: string;
  rtmpUrls: Record<string, string>;
  broadcastDelay: number;
  onStartWebcam: (ws: WebSocket) => Promise<void>;
  onStopWebcam: () => void;
  onHealthMessage?: (msg: PipelineHealthMsg) => void;
};

const MAX_RECONNECT_ATTEMPTS = 3;
const RECONNECT_DELAY_MS = 2000;

export function useBroadcastSocket(params: SocketParams) {
  const [isLive, setIsLive] = useState(false);
  const [sessionId, setSessionId] = useState<string | null>(null);
  const [interim, setInterim] = useState("");
  const [transcripts, setTranscripts] = useState<TranscriptEntry[]>([]);
  const [pipelineWarnings, setPipelineWarnings] = useState<string[]>([]);
  const { errors, addError, dismissError } = useDeduplicatedErrors();

  const wsRef = useRef<WebSocket | null>(null);
  const audioRef = useRef(new AudioPipeline());
  const reconnectCountRef = useRef(0);
  const startRef = useRef<(() => Promise<void>) | undefined>(undefined);

  const dispatch = { setInterim, setTranscripts };

  const handleMessage = useCallback((e: MessageEvent) => {
    if (typeof e.data !== "string") return;
    const msg = JSON.parse(e.data);

    switch (msg.type) {
      case "session:created":
        setSessionId(msg.id);
        reconnectCountRef.current = 0;
        break;
      case "interim":          setInterim(msg.transcript); break;
      case "final":            handleFinal(msg, dispatch); break;
      case "translation":      handleTranslation(msg, dispatch); break;
      case "chunk_translation": handleChunkTranslation(msg, dispatch); break;
      case "pipeline_warning": {
        const w = msg as { kind: string; lang: string; detail: string };
        const label = formatWarning(w.kind, w.lang, w.detail);
        setPipelineWarnings((prev) => [...prev, label]);
        break;
      }
      case "pipeline_health":
        params.onHealthMessage?.(msg as PipelineHealthMsg);
        break;
      case "error":            addError(msg.message); break;
    }
  }, [addError, params.onHealthMessage]);

  const start = useCallback(async () => {
    setSessionId(null); // clear previous session before establishing new one
    const available = filterTargetLanguages(params.sourceLang, params.targetLangs);
    if (available.length === 0) return;

    const ws = await connectWebSocket({ params, targets: available, onMessage: handleMessage, onError: addError });
    if (!ws) return;

    wsRef.current = ws;
    registerCloseHandler(ws, {
      addError,
      setIsLive,
      setSessionId,
      reconnectCountRef,
      startRef,
    });
    await configureRtmpStreams(ws, params);
    await startAudioCapture(ws, { audio: audioRef.current, deviceId: params.audioDeviceId });

    setIsLive(true);
    setTranscripts([]);
    setInterim("");
    setPipelineWarnings([]);
  }, [params, handleMessage, addError]);

  startRef.current = start;

  const stop = useCallback(() => {
    reconnectCountRef.current = MAX_RECONNECT_ATTEMPTS;
    params.onStopWebcam();
    audioRef.current.stop();
    wsRef.current?.close();
    wsRef.current = null;
    setIsLive(false);
    // Preserve sessionId after stop so DubbingPanel can show post-session.
    // Cleared at the start of the next session.
  }, [params]);

  return { isLive, sessionId, interim, transcripts, errors, pipelineWarnings, wsRef, start, stop, addError, dismissError };
}

// ── Helpers ──────────────────────────────────────────────

function filterTargetLanguages(sourceLang: string, targetLangs: string[]): string[] {
  return targetLangs.filter((l) => l !== sourceLang);
}

function buildWsUrl(params: SocketParams, targets: string[]): string {
  const base = appConfig.apiBaseUrl.replace(/^http/, "ws");
  const voiceDefaults = deriveVoiceDefaultLangs(params.voiceConfig).join(",");
  const genderMap = deriveVoiceGenderMap(params.voiceConfig);
  return `${base}/ws?sourceLang=${params.sourceLang}&targetLangs=${targets.join(",")}&tier=${params.tier}&ttsModel=${params.ttsModel}&ttsProvider=${params.ttsProvider}&voiceDefaultLangs=${voiceDefaults}&voiceGenderMap=${genderMap}`;
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
  reconnectCountRef: MutableRefObject<number>;
  startRef: MutableRefObject<(() => Promise<void>) | undefined>;
};

function registerCloseHandler(
  ws: WebSocket,
  { addError, setIsLive, setSessionId, reconnectCountRef, startRef }: CloseCallbacks,
): void {
  ws.onclose = (ev) => {
    if (ev.code === 1000) {
      // Normal stop — preserve sessionId for post-session UI (e.g. DubbingPanel).
      setIsLive(false);
      return;
    }

    // Error or reconnect path — clear sessionId since a new one is coming.
    setSessionId(null);

    if (reconnectCountRef.current < MAX_RECONNECT_ATTEMPTS) {
      reconnectCountRef.current += 1;
      addError(`Connection lost. Reconnecting... (attempt ${reconnectCountRef.current}/${MAX_RECONNECT_ATTEMPTS})`);
      setTimeout(() => startRef.current?.(), RECONNECT_DELAY_MS);
      return;
    }

    addError(`Connection lost (code ${ev.code}). Restart to reconnect.`);
    setIsLive(false);
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

const WARNING_LABELS: Record<string, string> = {
  stt_error: "STT error",
  stt_disconnected: "STT disconnected",
  tts_timeout: "TTS timed out",
  tts_failed: "TTS failed",
  rtmp_error: "RTMP error",
};

function formatWarning(kind: string, lang: string, detail: string): string {
  const label = WARNING_LABELS[kind] ?? kind;
  return lang ? `${label} (${lang}): ${detail}` : `${label}: ${detail}`;
}

function waitForOpen(ws: WebSocket): Promise<void> {
  return new Promise((resolve, reject) => {
    ws.onopen = () => resolve();
    ws.onerror = () => reject();
  });
}
