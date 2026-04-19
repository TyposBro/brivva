import { useCallback, useRef } from "react";
import * as api from "../data/api-client";
import type { HostAction } from "./reducer";

const VOICE_SAMPLE_SECONDS = 30;
const VOICE_SAMPLE_RATE = 44100;

type Recorder = {
  ctx: AudioContext;
  source: MediaStreamAudioSourceNode;
  processor: ScriptProcessorNode;
};

function mergePcmChunks(chunks: Int16Array[]): Uint8Array {
  const totalLen = chunks.reduce((s, c) => s + c.length, 0);
  const merged = new Int16Array(totalLen);
  let offset = 0;
  for (const chunk of chunks) {
    merged.set(chunk, offset);
    offset += chunk.length;
  }
  return new Uint8Array(merged.buffer);
}

function bytesToBase64(bytes: Uint8Array): string {
  let binary = "";
  for (let i = 0; i < bytes.length; i++) {
    binary += String.fromCharCode(bytes[i]);
  }
  return btoa(binary);
}

function float32ToInt16(float32: Float32Array): Int16Array {
  const int16 = new Int16Array(float32.length);
  for (let i = 0; i < float32.length; i++) {
    int16[i] = Math.max(-32768, Math.min(32767, Math.round(float32[i] * 32768)));
  }
  return int16;
}

/** Record a voice sample + upload it for cloning. */
export function useVoiceClone(
  dispatch: (a: HostAction) => void,
  getSessionId: () => string | null,
  getUserId: () => string | null,
) {
  const voicePcmRef = useRef<Int16Array[]>([]);
  const recorderRef = useRef<Recorder | null>(null);

  const stopVoiceRecording = useCallback(() => {
    const rec = recorderRef.current;
    if (!rec) return;
    rec.processor.disconnect();
    rec.source.disconnect();
    rec.ctx.close();
    recorderRef.current = null;

    const bytes = mergePcmChunks(voicePcmRef.current);
    voicePcmRef.current = [];
    const b64 = bytesToBase64(bytes);

    dispatch({ type: "voice_cloning" });
    const sessionId = getSessionId();
    const userId = getUserId();
    if (!sessionId) {
      dispatch({ type: "error", message: "Session ID required for voice cloning" });
      dispatch({ type: "skip_voice_setup" });
      return;
    }
    if (!userId) {
      dispatch({ type: "error", message: "User ID required for voice cloning" });
      dispatch({ type: "skip_voice_setup" });
      return;
    }

    void api
      .cloneSessionVoice(sessionId, { user_id: userId, audio_base64: b64 })
      .then(() => dispatch({ type: "voice_ready" }))
      .catch((err) => {
        dispatch({
          type: "error",
          message: err instanceof Error ? err.message : "Voice clone failed",
        });
        dispatch({ type: "skip_voice_setup" });
      });
  }, [dispatch, getSessionId, getUserId]);

  const startVoiceRecording = useCallback(async () => {
    voicePcmRef.current = [];
    try {
      const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
      const ctx = new AudioContext({ sampleRate: VOICE_SAMPLE_RATE });
      const source = ctx.createMediaStreamSource(stream);
      const processor = ctx.createScriptProcessor(4096, 1, 1);
      processor.onaudioprocess = (e) => {
        voicePcmRef.current.push(float32ToInt16(e.inputBuffer.getChannelData(0)));
      };
      source.connect(processor);
      processor.connect(ctx.destination);
      recorderRef.current = { ctx, source, processor };
      setTimeout(() => stopVoiceRecording(), VOICE_SAMPLE_SECONDS * 1000);
    } catch (err) {
      console.error("Voice recording failed:", err);
    }
  }, [stopVoiceRecording]);

  const skipVoiceSetup = useCallback(() => {
    dispatch({ type: "skip_voice_setup" });
  }, [dispatch]);

  return { startVoiceRecording, stopVoiceRecording, skipVoiceSetup };
}
