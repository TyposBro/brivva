import { useCallback, useRef, useState } from "react";

const VOICE_SAMPLE_RATE = 44100;
const PROGRESS_TICK_MS = 250;

const HIGH_FIDELITY_VOICE_CONSTRAINTS: MediaTrackConstraints = {
  channelCount: { ideal: 1 },
  sampleRate: { ideal: VOICE_SAMPLE_RATE },
  sampleSize: { ideal: 16 },
  echoCancellation: false,
  noiseSuppression: false,
  autoGainControl: false,
};

export interface VoiceRecorderOptions {
  minSec: number;
  maxSec: number;
  /** Fires when the hard cap is reached. The hook stops the recorder first
   *  and hands the encoded sample bytes to the caller. */
  onAutoStop?: (sampleBase64: string) => void;
}

interface ActiveRecorder {
  ctx: AudioContext;
  stream: MediaStream;
  source: MediaStreamAudioSourceNode;
  processor: ScriptProcessorNode;
}

function mergePcmChunks(chunks: Int16Array[]): Int16Array {
  const totalLen = chunks.reduce((s, c) => s + c.length, 0);
  const merged = new Int16Array(totalLen);
  let offset = 0;
  for (const chunk of chunks) {
    merged.set(chunk, offset);
    offset += chunk.length;
  }
  return merged;
}

// Wrap raw PCM samples in a minimal RIFF/WAVE container so the workers'
// validateVoiceSample probe (and ElevenLabs) can parse the upload. Without
// this header the bytes are rejected as "not a RIFF/WAVE container".
function encodeWav(samples: Int16Array, sampleRate: number): Uint8Array {
  const channels = 1;
  const bitsPerSample = 16;
  const bytesPerSample = bitsPerSample / 8;
  const dataSize = samples.length * bytesPerSample;
  const buf = new Uint8Array(44 + dataSize);
  const view = new DataView(buf.buffer);
  const ascii = (s: string, offset: number) => {
    for (let i = 0; i < s.length; i++) buf[offset + i] = s.charCodeAt(i);
  };
  ascii("RIFF", 0);
  view.setUint32(4, 36 + dataSize, true);
  ascii("WAVE", 8);
  ascii("fmt ", 12);
  view.setUint32(16, 16, true);
  view.setUint16(20, 1, true); // PCM
  view.setUint16(22, channels, true);
  view.setUint32(24, sampleRate, true);
  view.setUint32(28, sampleRate * channels * bytesPerSample, true);
  view.setUint16(32, channels * bytesPerSample, true);
  view.setUint16(34, bitsPerSample, true);
  ascii("data", 36);
  view.setUint32(40, dataSize, true);
  new Int16Array(buf.buffer, 44, samples.length).set(samples);
  return buf;
}

// Chunked base64 encode — a char-at-a-time string concat over a multi-MB
// audio buffer freezes the main thread for seconds (O(n) string allocs),
// which made voice-clone uploads silently hang before the POST could fire.
// String.fromCharCode.apply has a ~65k argument cap, so we batch per chunk.
function bytesToBase64(bytes: Uint8Array): string {
  const CHUNK = 0x8000;
  const parts: string[] = [];
  for (let i = 0; i < bytes.length; i += CHUNK) {
    const slice = bytes.subarray(i, i + CHUNK);
    parts.push(String.fromCharCode.apply(null, slice as unknown as number[]));
  }
  return btoa(parts.join(""));
}

function float32ToInt16(float32: Float32Array): Int16Array {
  const int16 = new Int16Array(float32.length);
  for (let i = 0; i < float32.length; i++) {
    int16[i] = Math.max(-32768, Math.min(32767, Math.round(float32[i] * 32768)));
  }
  return int16;
}

/** PCM voice-sample recorder. Owns the AudioContext, accumulates PCM chunks,
 *  and reports elapsed time so the caller can render progress. Auto-stops
 *  when `maxSec` is reached. The hook stays presentation-agnostic — UI lives
 *  in the caller. */
export function useVoiceRecorder({ minSec, maxSec, onAutoStop }: VoiceRecorderOptions) {
  const pcmRef = useRef<Int16Array[]>([]);
  const recorderRef = useRef<ActiveRecorder | null>(null);
  const tickRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const [elapsedSec, setElapsedSec] = useState(0);
  const [isRecording, setIsRecording] = useState(false);

  const stopTick = useCallback(() => {
    if (tickRef.current) {
      clearInterval(tickRef.current);
      tickRef.current = null;
    }
  }, []);

  const stop = useCallback((): string | null => {
    const rec = recorderRef.current;
    if (!rec) return null;
    stopTick();
    setIsRecording(false);
    rec.processor.disconnect();
    rec.source.disconnect();
    rec.stream.getTracks().forEach((track) => track.stop());
    rec.ctx.close();
    recorderRef.current = null;
    const samples = mergePcmChunks(pcmRef.current);
    pcmRef.current = [];
    return bytesToBase64(encodeWav(samples, VOICE_SAMPLE_RATE));
  }, [stopTick]);

  const start = useCallback(async () => {
    if (recorderRef.current) return;
    pcmRef.current = [];
    setElapsedSec(0);
    const stream = await navigator.mediaDevices.getUserMedia({
      audio: HIGH_FIDELITY_VOICE_CONSTRAINTS,
    });
    const ctx = new AudioContext({ sampleRate: VOICE_SAMPLE_RATE });
    const source = ctx.createMediaStreamSource(stream);
    const processor = ctx.createScriptProcessor(4096, 1, 1);
    processor.onaudioprocess = (e) => {
      pcmRef.current.push(float32ToInt16(e.inputBuffer.getChannelData(0)));
    };
    source.connect(processor);
    processor.connect(ctx.destination);
    recorderRef.current = { ctx, stream, source, processor };
    setIsRecording(true);

    const startedAt = Date.now();
    tickRef.current = setInterval(() => {
      const sec = Math.floor((Date.now() - startedAt) / 1000);
      setElapsedSec(sec);
      if (sec >= maxSec) {
        const b64 = stop();
        if (b64 !== null) onAutoStop?.(b64);
      }
    }, PROGRESS_TICK_MS);
  }, [maxSec, onAutoStop, stop]);

  return { start, stop, elapsedSec, isRecording, minSec, maxSec };
}
