import { useState, useRef, useCallback } from "react";

const WORKER_URL = import.meta.env.VITE_WORKER_URL ?? "http://localhost:8787";
const WS_URL = WORKER_URL.replace(/^http/, "ws");

const SAMPLE_RATE = 16000; // Nova-3 expects 16kHz PCM
const BUFFER_SIZE = 4096;  // ScriptProcessor chunk size

export type Utterance = {
  id: number;
  transcript: string;  // English (finalized)
  translation: string; // Spanish
};

export type RealtimeStatus = "idle" | "connecting" | "listening" | "processing";

export function useRealtimeTranslation() {
  const [status, setStatus] = useState<RealtimeStatus>("idle");
  const [liveTranscript, setLiveTranscript] = useState("");
  const [utterances, setUtterances] = useState<Utterance[]>([]);
  const [analyser, setAnalyser] = useState<AnalyserNode | null>(null);

  const wsRef = useRef<WebSocket | null>(null);
  const audioCtxRef = useRef<AudioContext | null>(null);
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const processorRef = useRef<any>(null);
  const ttsChunksRef = useRef<ArrayBuffer[]>([]);

  const start = useCallback(async () => {
    setStatus("connecting");
    setUtterances([]);
    setLiveTranscript("");

    const stream = await navigator.mediaDevices.getUserMedia({ audio: true });

    // Web Audio at 16kHz — matches Nova-3 linear16 encoding
    const audioCtx = new AudioContext({ sampleRate: SAMPLE_RATE });
    audioCtxRef.current = audioCtx;

    const source = audioCtx.createMediaStreamSource(stream);

    // Analyser for waveform visualisation only
    const analyserNode = audioCtx.createAnalyser();
    analyserNode.fftSize = 512;
    source.connect(analyserNode);
    setAnalyser(analyserNode);

    const ws = new WebSocket(`${WS_URL}/api/realtime`);
    ws.binaryType = "arraybuffer";
    wsRef.current = ws;

    ws.onopen = () => {
      // ScriptProcessor captures raw PCM and streams it immediately
      // (deprecated but widely supported; AudioWorklet is the modern alternative)
      const processor = audioCtx.createScriptProcessor(BUFFER_SIZE, 1, 1);
      processorRef.current = processor;

      processor.onaudioprocess = (e) => {
        if (ws.readyState !== WebSocket.OPEN) return;
        const float32 = e.inputBuffer.getChannelData(0);
        // Convert Float32 [-1,1] → Int16 [-32768,32767]
        const int16 = new Int16Array(float32.length);
        for (let i = 0; i < float32.length; i++) {
          int16[i] = Math.max(-32768, Math.min(32767, float32[i] * 32768));
        }
        ws.send(int16.buffer);
      };

      source.connect(processor);
      processor.connect(audioCtx.destination);
      setStatus("listening");
    };

    ws.onmessage = (event) => {
      if (typeof event.data === "string") {
        const msg = JSON.parse(event.data) as {
          type: string;
          transcript?: string;
          text?: string;
          utteranceId?: number;
        };

        if (msg.type === "interim") {
          setLiveTranscript(msg.transcript ?? "");
          setStatus("listening");
        } else if (msg.type === "final") {
          setUtterances((prev) => [
            ...prev,
            { id: msg.utteranceId!, transcript: msg.transcript ?? "", translation: "" },
          ]);
          setLiveTranscript("");
          setStatus("processing");
        } else if (msg.type === "translation") {
          setUtterances((prev) =>
            prev.map((u) =>
              u.id === msg.utteranceId ? { ...u, translation: msg.text ?? "" } : u
            )
          );
          setStatus("listening");
        } else if (msg.type === "tts_start") {
          ttsChunksRef.current = [];
        } else if (msg.type === "tts_end") {
          const blob = new Blob(ttsChunksRef.current, { type: "audio/mpeg" });
          const url = URL.createObjectURL(blob);
          const audio = new Audio(url);
          audio.onended = () => URL.revokeObjectURL(url);
          audio.play().catch(console.error);
          ttsChunksRef.current = [];
        } else if (msg.type === "error") {
          console.error("Worker error:", msg);
          setStatus("idle");
        }
      } else if (event.data instanceof ArrayBuffer) {
        ttsChunksRef.current.push(event.data);
      }
    };

    const cleanup = () => {
      processorRef.current?.disconnect();
      processorRef.current = null;
      stream.getTracks().forEach((t) => t.stop());
      audioCtxRef.current?.close();
      setAnalyser(null);
      setLiveTranscript("");
      setStatus("idle");
    };

    ws.onclose = cleanup;
    ws.onerror = () => ws.close();
  }, []);

  const stop = useCallback(() => {
    wsRef.current?.close();
  }, []);

  return { status, liveTranscript, utterances, analyser, start, stop };
}
