import { useState, useRef, useCallback } from "react";

const WORKER_URL = import.meta.env.VITE_WORKER_URL ?? "http://localhost:8787";
const WS_URL = WORKER_URL.replace(/^http/, "ws");

const SAMPLE_RATE = 16000; // Nova-3 expects 16kHz PCM
const BUFFER_SIZE = 4096;  // ScriptProcessor chunk size

export type Timing = {
  sttStartAt?: number;    // first interim received (proxy for speech start)
  finalAt: number;
  translationAt?: number;
  translateMs?: number;   // M2M100 duration on CF edge
  ttsStartAt?: number;
  ttsEndAt?: number;
  ttsMs?: number;         // Kokoro TTS generation duration
};

export type Utterance = {
  id: number;
  transcript: string;   // English (finalized)
  translation: string;  // Japanese
  timing: Timing;
};

export type RealtimeStatus = "idle" | "connecting" | "listening" | "processing";

type LogEntry = {
  t: string;
  ms: number;
  event: string;
  [key: string]: unknown;
};

// One entry per utterance — buffered while waiting, streamed while playing
type TtsEntry = {
  utteranceId: number;
  chunks: ArrayBuffer[];  // buffered until this entry starts playing
  done: boolean;          // tts_end received
};

type PlaybackState = {
  entry: TtsEntry;
  audio: HTMLAudioElement;
  mediaSource: MediaSource;
  sourceBuffer: SourceBuffer;
  appendQueue: ArrayBuffer[];  // chunks waiting to be appended to MSE
  objectUrl: string;
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
      const blob = new Blob([text], { type: "application/json" });
      const url = URL.createObjectURL(blob);
      window.open(url, "_blank");
    });
  }, []);

  const wsRef = useRef<WebSocket | null>(null);
  const audioCtxRef = useRef<AudioContext | null>(null);
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const processorRef = useRef<any>(null);

  const interimStartRef = useRef<number | null>(null);

  // TTS streaming via MSE — FIFO, no overlap, plays first chunk immediately
  const receivingEntryRef = useRef<TtsEntry | null>(null);  // entry currently receiving WS chunks
  const ttsQueueRef = useRef<TtsEntry[]>([]);               // ordered playback queue
  const isTtsPlayingRef = useRef(false);
  const playbackRef = useRef<PlaybackState | null>(null);
  const tryPlayNextRef = useRef<() => void>(() => {});      // ref to break startPlayback↔tryPlayNext cycle

  // Append next chunk from queue to MSE source buffer (called on updateend + new chunk)
  function appendNextFromQueue() {
    const pb = playbackRef.current;
    if (!pb || !pb.sourceBuffer) return;
    if (pb.sourceBuffer.updating) return;
    if (pb.appendQueue.length > 0) {
      pb.sourceBuffer.appendBuffer(pb.appendQueue.shift()!);
    } else if (pb.entry.done && pb.mediaSource.readyState === "open") {
      pb.mediaSource.endOfStream();
    }
  }

  // Begin MSE streaming playback for an entry
  function startPlayback(entry: TtsEntry) {
    isTtsPlayingRef.current = true;

    const mediaSource = new MediaSource();
    const objectUrl = URL.createObjectURL(mediaSource);
    const audio = new Audio(objectUrl);

    const pb: PlaybackState = {
      entry,
      audio,
      mediaSource,
      sourceBuffer: null as unknown as SourceBuffer, // assigned in sourceopen
      appendQueue: [...entry.chunks],  // snapshot all buffered chunks
      objectUrl,
    };
    entry.chunks = []; // future chunks go directly to pb.appendQueue
    playbackRef.current = pb;

    mediaSource.addEventListener("sourceopen", () => {
      try {
        const sb = mediaSource.addSourceBuffer("audio/mpeg");
        pb.sourceBuffer = sb;
        sb.addEventListener("updateend", appendNextFromQueue);
        appendNextFromQueue(); // start draining the initial buffer
      } catch (err) {
        console.error("MSE setup error:", err);
      }
    });

    const onDone = () => {
      URL.revokeObjectURL(objectUrl);
      playbackRef.current = null;
      isTtsPlayingRef.current = false;
      ttsQueueRef.current.shift();
      tryPlayNextRef.current();
    };
    audio.onended = onDone;
    audio.onerror = () => { console.error("Audio playback error"); onDone(); };

    audio.play().catch(console.error);
  }

  // Start next queued entry if idle and chunks are available
  function tryPlayNext() {
    if (isTtsPlayingRef.current) return;
    const queue = ttsQueueRef.current;
    if (queue.length === 0) return;
    const entry = queue[0];
    if (entry.chunks.length === 0 && !entry.done) return; // wait for first chunk
    startPlayback(entry);
  }

  tryPlayNextRef.current = tryPlayNext; // keep ref fresh each render

  const start = useCallback(async () => {
    logRef.current = [];
    sessionStartRef.current = Date.now();
    log("SESSION_START");
    ttsQueueRef.current = [];
    receivingEntryRef.current = null;
    isTtsPlayingRef.current = false;
    playbackRef.current = null;
    setStatus("connecting");
    setUtterances([]);
    setLiveTranscript("");

    const stream = await navigator.mediaDevices.getUserMedia({ audio: true });

    const audioCtx = new AudioContext({ sampleRate: SAMPLE_RATE });
    audioCtxRef.current = audioCtx;

    const source = audioCtx.createMediaStreamSource(stream);

    const analyserNode = audioCtx.createAnalyser();
    analyserNode.fftSize = 512;
    source.connect(analyserNode);
    setAnalyser(analyserNode);

    const ws = new WebSocket(`${WS_URL}/api/realtime`);
    ws.binaryType = "arraybuffer";
    wsRef.current = ws;

    ws.onopen = () => {
      const processor = audioCtx.createScriptProcessor(BUFFER_SIZE, 1, 1);
      processorRef.current = processor;

      processor.onaudioprocess = (e) => {
        if (ws.readyState !== WebSocket.OPEN) return;
        const float32 = e.inputBuffer.getChannelData(0);
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
          if (interimStartRef.current === null) interimStartRef.current = Date.now();
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
          // Create entry, push to queue, try to start playing immediately
          const entry: TtsEntry = { utteranceId: msg.utteranceId!, chunks: [], done: false };
          receivingEntryRef.current = entry;
          ttsQueueRef.current.push(entry);
          setUtterances((prev) =>
            prev.map((u) => {
              if (u.id !== msg.utteranceId) return u;
              log("TTS_START", { utteranceId: msg.utteranceId, msFromFinal: Date.now() - u.timing.finalAt });
              return { ...u, timing: { ...u.timing, ttsStartAt: Date.now() } };
            })
          );

        } else if (msg.type === "tts_end") {
          // Mark done → MSE will call endOfStream after last chunk is appended
          const entry = receivingEntryRef.current;
          if (entry && entry.utteranceId === msg.utteranceId) {
            entry.done = true;
            receivingEntryRef.current = null;
            appendNextFromQueue(); // trigger endOfStream if all chunks already appended
          }
          setUtterances((prev) =>
            prev.map((u) => {
              if (u.id !== msg.utteranceId) return u;
              const ttsEndAt = Date.now();
              const totalMs = ttsEndAt - u.timing.finalAt;
              log("TTS_END", { utteranceId: msg.utteranceId, ttsMs: msg.ttsMs, totalMs });
              return { ...u, timing: { ...u.timing, ttsEndAt, ttsMs: msg.ttsMs } };
            })
          );

        } else if (msg.type === "error") {
          console.error("Worker error:", msg);
          log("ERROR", { message: msg });
          setStatus("idle");
        }

      } else if (event.data instanceof ArrayBuffer) {
        const entry = receivingEntryRef.current;
        if (!entry) return;

        const pb = playbackRef.current;
        if (pb && pb.entry === entry) {
          // This entry is currently playing — append directly to MSE
          pb.appendQueue.push(event.data);
          appendNextFromQueue();
        } else {
          // Not playing yet — buffer until it's this entry's turn
          entry.chunks.push(event.data);
          tryPlayNextRef.current(); // start playback if this is front-of-queue
        }
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

  const clear = useCallback(() => {
    if (playbackRef.current) {
      playbackRef.current.audio.pause();
      playbackRef.current.audio.src = "";
      URL.revokeObjectURL(playbackRef.current.objectUrl);
      playbackRef.current = null;
    }
    isTtsPlayingRef.current = false;
    ttsQueueRef.current = [];
    receivingEntryRef.current = null;
    interimStartRef.current = null;
    logRef.current = [];
    sessionStartRef.current = Date.now();
    setUtterances([]);
    setLiveTranscript("");
    log("SESSION_CLEAR");
  }, []);

  return { status, liveTranscript, utterances, analyser, start, stop, clear, copyLog };
}
