import { useState, useRef, useCallback } from "react";

const WORKER_URL = import.meta.env.VITE_WORKER_URL ?? "http://localhost:8787";
const WS_URL = WORKER_URL.replace(/^http/, "ws");

const SAMPLE_RATE = 16000; // Nova-3 expects 16kHz PCM
const BUFFER_SIZE = 4096;  // ScriptProcessor chunk size

export type Timing = {
  sttStartAt?: number;   // first interim received (proxy for speech start)
  finalAt: number;
  translationAt?: number;
  translateMs?: number;  // M2M100 duration on CF edge
  ttsStartAt?: number;
  ttsEndAt?: number;
  ttsMs?: number;        // TTS generation duration on CF edge
};

export type Utterance = {
  id: number;
  transcript: string;   // English (finalized)
  translation: string;  // Spanish
  timing: Timing;
};

export type RealtimeStatus = "idle" | "connecting" | "listening" | "processing";

type LogEntry = {
  t: string;         // ISO timestamp
  ms: number;        // ms since session start
  event: string;
  [key: string]: unknown;
};

export function useRealtimeTranslation() {
  const [status, setStatus] = useState<RealtimeStatus>("idle");
  const [liveTranscript, setLiveTranscript] = useState("");
  const [utterances, setUtterances] = useState<Utterance[]>([]);
  const [analyser, setAnalyser] = useState<AnalyserNode | null>(null);

  const logRef = useRef<LogEntry[]>([]);
  const sessionStartRef = useRef<number>(0);

  const log = (event: string, data?: Record<string, unknown>) => {
    logRef.current.push({
      t: new Date().toISOString(),
      ms: Date.now() - sessionStartRef.current,
      event,
      ...data,
    });
  };

  const copyLog = useCallback(() => {
    const text = JSON.stringify(logRef.current, null, 2);
    navigator.clipboard.writeText(text).catch(() => {
      // fallback: open in new tab
      const blob = new Blob([text], { type: "application/json" });
      const url = URL.createObjectURL(blob);
      window.open(url, "_blank");
    });
  }, []);

  const wsRef = useRef<WebSocket | null>(null);
  const audioCtxRef = useRef<AudioContext | null>(null);
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const processorRef = useRef<any>(null);

  // Tracks first interim arrival for current utterance (STT phase start)
  const interimStartRef = useRef<number | null>(null);

  // TTS queue — collect each clip fully, play sequentially (no overlap)
  const ttsQueueRef = useRef<ArrayBuffer[][]>([]);
  const currentTtsChunksRef = useRef<ArrayBuffer[]>([]);
  const isTtsPlayingRef = useRef(false);

  const playNextTts = useCallback(() => {
    if (isTtsPlayingRef.current || ttsQueueRef.current.length === 0) return;
    isTtsPlayingRef.current = true;
    const chunks = ttsQueueRef.current.shift()!;

    // Concatenate all chunks into one buffer
    const totalBytes = chunks.reduce((n, c) => n + c.byteLength, 0);
    const combined = new Uint8Array(totalBytes);
    let offset = 0;
    for (const chunk of chunks) {
      combined.set(new Uint8Array(chunk), offset);
      offset += chunk.byteLength;
    }

    // decodeAudioData auto-detects format — works on all browsers regardless of MIME type
    const ctx = new AudioContext();
    ctx.decodeAudioData(combined.buffer.slice(0)).then((audioBuffer) => {
      const source = ctx.createBufferSource();
      source.buffer = audioBuffer;
      source.connect(ctx.destination);
      source.onended = () => {
        ctx.close();
        isTtsPlayingRef.current = false;
        playNextTts();
      };
      source.start();
    }).catch((err) => {
      console.error("TTS decode error:", err);
      ctx.close();
      isTtsPlayingRef.current = false;
      playNextTts();
    });
  }, []);

  const start = useCallback(async () => {
    logRef.current = [];
    sessionStartRef.current = Date.now();
    log("SESSION_START");
    ttsQueueRef.current = [];
    currentTtsChunksRef.current = [];
    isTtsPlayingRef.current = false;
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
          translateMs?: number;
          ttsMs?: number;
        };

        if (msg.type === "interim") {
          interimStartRef.current = Date.now(); // update every time → last interim on FINAL
          setLiveTranscript(msg.transcript ?? "");
          setStatus("listening");
          log("INTERIM", { transcript: msg.transcript });
        } else if (msg.type === "final") {
          const finalAt = Date.now();
          const sttStartAt = interimStartRef.current ?? finalAt;
          interimStartRef.current = null;
          setUtterances((prev) => [
            ...prev,
            { id: msg.utteranceId!, transcript: msg.transcript ?? "", translation: "", timing: { sttStartAt, finalAt } },
          ]);
          setLiveTranscript("");
          setStatus("processing");
          log("FINAL", { utteranceId: msg.utteranceId, transcript: msg.transcript });
        } else if (msg.type === "translation") {
          const translationAt = Date.now();
          setUtterances((prev) =>
            prev.map((u) => {
              if (u.id !== msg.utteranceId) return u;
              const totalMs = translationAt - u.timing.finalAt;
              log("TRANSLATION", { utteranceId: msg.utteranceId, text: msg.text, cfMs: msg.translateMs, totalMs });
              return { ...u, translation: msg.text ?? "", timing: { ...u.timing, translationAt, translateMs: msg.translateMs } };
            })
          );
          setStatus("listening");
        } else if (msg.type === "tts_start") {
          setUtterances((prev) =>
            prev.map((u) => {
              if (u.id !== msg.utteranceId) return u;
              log("TTS_START", { utteranceId: msg.utteranceId, msFromFinal: Date.now() - u.timing.finalAt });
              return { ...u, timing: { ...u.timing, ttsStartAt: Date.now() } };
            })
          );
          currentTtsChunksRef.current = [];
        } else if (msg.type === "tts_end") {
          setUtterances((prev) =>
            prev.map((u) => {
              if (u.id !== msg.utteranceId) return u;
              const ttsEndAt = Date.now();
              const totalMs = ttsEndAt - u.timing.finalAt;
              log("TTS_END", { utteranceId: msg.utteranceId, cfMs: msg.ttsMs, totalMs });
              return { ...u, timing: { ...u.timing, ttsEndAt, ttsMs: msg.ttsMs } };
            })
          );
          // Enqueue completed clip and play when previous finishes
          ttsQueueRef.current.push(currentTtsChunksRef.current);
          currentTtsChunksRef.current = [];
          playNextTts();
        } else if (msg.type === "error") {
          console.error("Worker error:", msg);
          log("ERROR", { message: msg });
          setStatus("idle");
        }
      } else if (event.data instanceof ArrayBuffer) {
        currentTtsChunksRef.current.push(event.data);
      }
    };

    const cleanup = () => {
      log("SESSION_END");
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

  // Reset session data without closing the connection (accounts for cold starts)
  const clear = useCallback(() => {
    setUtterances([]);
    setLiveTranscript("");
    interimStartRef.current = null;
    ttsQueueRef.current = [];
    currentTtsChunksRef.current = [];
    isTtsPlayingRef.current = false;
    logRef.current = [];
    sessionStartRef.current = Date.now();
    log("SESSION_CLEAR");
  }, []);

  return { status, liveTranscript, utterances, analyser, start, stop, clear, copyLog };
}
