import { useRef, useCallback } from "react";
import type { Dispatch } from "react";
import type { HostAction } from "../state/reducer";
import { float32ToInt16, mergePcmChunks, pcmToBase64 } from "../../../shared/audio/pcm";
import { VOICE_SAMPLE_RATE, VOICE_SAMPLE_SECONDS, VOICE_BUFFER_SIZE } from "../constants";

type VoiceRecordingDeps = {
  dispatch: Dispatch<HostAction>;
  sendJson: (msg: object) => void;
};

export function useVoiceRecording({ dispatch, sendJson }: VoiceRecordingDeps) {
  const pcmChunks = useRef<Int16Array[]>([]);
  const recorder = useRef<{ ctx: AudioContext; source: MediaStreamAudioSourceNode; processor: ScriptProcessorNode } | null>(null);

  const stopVoiceRecording = useCallback(() => {
    const rec = recorder.current;
    if (!rec) return;

    rec.processor.disconnect();
    rec.source.disconnect();
    rec.ctx.close();
    recorder.current = null;

    const merged = mergePcmChunks(pcmChunks.current);
    pcmChunks.current = [];

    dispatch({ type: "voice_cloning" });
    sendJson({ type: "voice:sample", data: pcmToBase64(merged) });
  }, [dispatch, sendJson]);

  const startVoiceRecording = useCallback(async () => {
    pcmChunks.current = [];
    try {
      const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
      const ctx = new AudioContext({ sampleRate: VOICE_SAMPLE_RATE });
      const source = ctx.createMediaStreamSource(stream);
      const processor = ctx.createScriptProcessor(VOICE_BUFFER_SIZE, 1, 1);

      processor.onaudioprocess = (e) => {
        pcmChunks.current.push(float32ToInt16(e.inputBuffer.getChannelData(0)));
      };

      source.connect(processor);
      processor.connect(ctx.destination);
      recorder.current = { ctx, source, processor };

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
