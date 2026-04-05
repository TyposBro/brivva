import { useState, useRef, useCallback } from "react";
import { API_BASE } from "../../../shared/api/client";
import { AudioPipeline } from "../../../lib/AudioPipeline";
import { AUDIO_TAG, type TranscriptEntry, type TranslationTier } from "../constants";

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

  const handleMessage = useCallback((e: MessageEvent) => {
    if (typeof e.data !== "string") return;
    const msg = JSON.parse(e.data);

    switch (msg.type) {
      case "session:created":  setSessionId(msg.id); break;
      case "interim":          setInterim(msg.transcript); break;
      case "final":            handleFinal(msg, setInterim, setTranscripts); break;
      case "translation":      handleTranslation(msg, setTranscripts); break;
      case "chunk_translation": handleChunkTranslation(msg, setTranscripts); break;
      case "error":            addError(msg.message); break;
    }
  }, [addError]);

  const start = useCallback(async () => {
    const { sourceLang, targetLangs, tier, ttsModel, audioDeviceId, rtmpUrls, broadcastDelay, onStartWebcam } = params;
    const available = targetLangs.filter((l) => l !== sourceLang);
    if (available.length === 0) return;

    const wsUrl = `${API_BASE.replace(/^http/, "ws")}/ws?sourceLang=${sourceLang}&targetLangs=${available.join(",")}&tier=${tier}&ttsModel=${ttsModel}`;
    const ws = new WebSocket(wsUrl);
    ws.binaryType = "arraybuffer";
    wsRef.current = ws;
    ws.onmessage = handleMessage;

    try {
      await waitForOpen(ws);
    } catch {
      addError("Cannot connect — is the backend running?");
      wsRef.current = null;
      return;
    }

    ws.onclose = (ev) => {
      if (ev.code !== 1000) addError(`Connection lost (code ${ev.code}). Restart to reconnect.`);
      setIsLive(false);
      setSessionId(null);
    };
    ws.onerror = () => {};

    const streams = Object.entries(rtmpUrls)
      .filter(([, url]) => url.trim())
      .map(([lang, url]) => ({ lang, url: url.trim() }));

    if (streams.length > 0) {
      await onStartWebcam(ws);
      ws.send(JSON.stringify({ type: "rtmp:config", streams, broadcastDelay }));
    }

    await audioRef.current.start((buffer) => {
      if (ws.readyState === WebSocket.OPEN) {
        const tagged = new Uint8Array(buffer.byteLength + 1);
        tagged[0] = AUDIO_TAG;
        tagged.set(new Uint8Array(buffer), 1);
        ws.send(tagged.buffer);
      }
    }, audioDeviceId || undefined);

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

// ── Message handlers ─────────────────────────────────────

function handleFinal(
  msg: { utteranceId: number; transcript: string },
  setInterim: (s: string) => void,
  setTranscripts: React.Dispatch<React.SetStateAction<TranscriptEntry[]>>,
) {
  setInterim("");
  setTranscripts((prev) => [...prev, { id: msg.utteranceId, text: msg.transcript, translations: {} }]);
}

function handleTranslation(
  msg: { utteranceId: number; lang: string; text: string },
  setTranscripts: React.Dispatch<React.SetStateAction<TranscriptEntry[]>>,
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
  setTranscripts: React.Dispatch<React.SetStateAction<TranscriptEntry[]>>,
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
