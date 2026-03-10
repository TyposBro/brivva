import { useState, useRef, useCallback } from "react";

const SILENCE_THRESHOLD = 10; // RMS threshold (0–128 scale)
const SILENCE_DURATION_MS = 900; // ms of silence → end of utterance

const WORKER_URL = import.meta.env.VITE_WORKER_URL ?? "http://localhost:8787";
const WS_URL = WORKER_URL.replace(/^http/, "ws"); // http→ws, https→wss

export type Utterance = {
  id: number;
  transcription: string;
  translation: string;
};

export type RealtimeStatus = "idle" | "connecting" | "listening" | "processing";

export function useRealtimeTranslation() {
  const [status, setStatus] = useState<RealtimeStatus>("idle");
  const [utterances, setUtterances] = useState<Utterance[]>([]);
  const [analyser, setAnalyser] = useState<AnalyserNode | null>(null);

  const wsRef = useRef<WebSocket | null>(null);
  const recorderRef = useRef<MediaRecorder | null>(null);
  const audioCtxRef = useRef<AudioContext | null>(null);
  const rafRef = useRef<number>(0);
  const silenceTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const isSpeakingRef = useRef(false);
  const hasMediaChunksRef = useRef(false);

  const triggerProcess = useCallback(() => {
    if (wsRef.current?.readyState === WebSocket.OPEN && hasMediaChunksRef.current) {
      wsRef.current.send(JSON.stringify({ type: "process" }));
      hasMediaChunksRef.current = false;
      setStatus("processing");
    }
  }, []);

  const startSilenceDetection = useCallback(
    (node: AnalyserNode) => {
      const buf = new Uint8Array(node.frequencyBinCount);

      const tick = () => {
        rafRef.current = requestAnimationFrame(tick);
        node.getByteTimeDomainData(buf);

        // RMS of time-domain samples (centered at 128)
        const rms = Math.sqrt(buf.reduce((s, v) => s + (v - 128) ** 2, 0) / buf.length);
        const speaking = rms > SILENCE_THRESHOLD;

        if (speaking) {
          isSpeakingRef.current = true;
          if (silenceTimerRef.current) {
            clearTimeout(silenceTimerRef.current);
            silenceTimerRef.current = null;
          }
        } else if (isSpeakingRef.current && !silenceTimerRef.current) {
          silenceTimerRef.current = setTimeout(() => {
            isSpeakingRef.current = false;
            silenceTimerRef.current = null;
            triggerProcess();
            setStatus((s) => (s === "processing" ? s : "listening"));
          }, SILENCE_DURATION_MS);
        }
      };

      tick();
    },
    [triggerProcess]
  );

  const start = useCallback(
    async (sourceLang: string) => {
      setStatus("connecting");
      setUtterances([]);

      const stream = await navigator.mediaDevices.getUserMedia({ audio: true });

      const audioCtx = new AudioContext();
      audioCtxRef.current = audioCtx;
      const source = audioCtx.createMediaStreamSource(stream);
      const analyserNode = audioCtx.createAnalyser();
      analyserNode.fftSize = 512;
      source.connect(analyserNode);
      setAnalyser(analyserNode);

      const ws = new WebSocket(`${WS_URL}/api/realtime`);
      wsRef.current = ws;

      const mimeType = MediaRecorder.isTypeSupported("audio/webm;codecs=opus")
        ? "audio/webm;codecs=opus"
        : "audio/webm";

      const recorder = new MediaRecorder(stream, { mimeType });
      recorderRef.current = recorder;

      recorder.ondataavailable = (e) => {
        if (e.data.size > 0 && ws.readyState === WebSocket.OPEN) {
          e.data.arrayBuffer().then((buf) => {
            ws.send(buf);
            hasMediaChunksRef.current = true;
          });
        }
      };

      ws.onopen = () => {
        ws.send(JSON.stringify({ type: "config", sourceLang }));
        recorder.start(250);
        setStatus("listening");
        startSilenceDetection(analyserNode);
      };

      ws.onmessage = (event) => {
        if (typeof event.data !== "string") return;
        const msg = JSON.parse(event.data) as {
          type: string;
          transcription?: string;
          translation?: string;
        };
        if (msg.type === "result") {
          setUtterances((prev) => [
            ...prev,
            {
              id: Date.now(),
              transcription: msg.transcription ?? "",
              translation: msg.translation ?? "",
            },
          ]);
          setStatus("listening");
        }
      };

      const cleanup = () => {
        stream.getTracks().forEach((t) => t.stop());
        audioCtxRef.current?.close();
        cancelAnimationFrame(rafRef.current);
        if (silenceTimerRef.current) clearTimeout(silenceTimerRef.current);
        silenceTimerRef.current = null;
        isSpeakingRef.current = false;
        hasMediaChunksRef.current = false;
        setAnalyser(null);
        setStatus("idle");
      };

      ws.onclose = cleanup;
      ws.onerror = () => ws.close();
    },
    [startSilenceDetection]
  );

  const stop = useCallback(() => {
    recorderRef.current?.stop();
    wsRef.current?.close();
  }, []);

  return { status, utterances, analyser, start, stop };
}
