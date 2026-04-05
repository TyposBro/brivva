import { useState, useEffect, useCallback } from "react";
import { API_BASE } from "../../../core/api/client";
import { float32ToInt16, mergePcmChunks } from "../../../core/audio/pcm";
import { CLONE_DURATION_SEC, CLONE_SAMPLE_RATE, CLONE_BUFFER_SIZE, CLONE_PROGRESS_INTERVAL_MS } from "../constants";

export function useVoiceClone(onError: (msg: string) => void) {
  const [isCloning, setIsCloning] = useState(false);
  const [cloneProgress, setCloneProgress] = useState(0);
  const [voiceReady, setVoiceReady] = useState(false);

  const checkVoiceStatus = useCallback(() => {
    fetch(`${API_BASE}/api/voice`)
      .then((r) => r.json())
      .then((data) => { if (data.active) setVoiceReady(true); })
      .catch(() => {});
  }, []);

  useEffect(() => { checkVoiceStatus(); }, [checkVoiceStatus]);

  const cloneVoice = useCallback(async () => {
    setIsCloning(true);
    setCloneProgress(0);
    setVoiceReady(false);

    const { chunks, cleanup } = await recordAudio();
    const timer = startProgressTimer(setCloneProgress);

    await waitForDuration();

    clearInterval(timer);
    cleanup();

    const pcm = mergePcmChunks(chunks);
    setCloneProgress(1);

    await uploadVoice(pcm, { onReady: setVoiceReady, onError });
    setIsCloning(false);
  }, [onError]);

  return { isCloning, cloneProgress, voiceReady, setVoiceReady, cloneVoice };
}

async function recordAudio() {
  const chunks: Int16Array[] = [];
  const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
  const ctx = new AudioContext({ sampleRate: CLONE_SAMPLE_RATE });
  const source = ctx.createMediaStreamSource(stream);
  const processor = ctx.createScriptProcessor(CLONE_BUFFER_SIZE, 1, 1);

  processor.onaudioprocess = (e) => {
    chunks.push(float32ToInt16(e.inputBuffer.getChannelData(0)));
  };

  source.connect(processor);
  processor.connect(ctx.destination);

  const cleanup = () => {
    processor.disconnect();
    stream.getTracks().forEach((t) => t.stop());
    ctx.close();
  };

  return { chunks, cleanup };
}

function startProgressTimer(setProgress: (p: number) => void) {
  const start = Date.now();
  return window.setInterval(() => {
    const elapsed = (Date.now() - start) / 1000;
    setProgress(Math.min(elapsed / CLONE_DURATION_SEC, 1));
  }, CLONE_PROGRESS_INTERVAL_MS);
}

function waitForDuration() {
  return new Promise((resolve) => setTimeout(resolve, CLONE_DURATION_SEC * 1000));
}

type UploadCallbacks = {
  onReady: (v: boolean) => void;
  onError: (msg: string) => void;
};

async function uploadVoice(pcm: Int16Array, { onReady, onError }: UploadCallbacks) {
  try {
    const resp = await fetch(`${API_BASE}/api/voice/clone`, {
      method: "POST",
      headers: { "Content-Type": "application/octet-stream" },
      body: new Uint8Array(pcm.buffer) as unknown as BodyInit,
    });
    if (resp.ok) onReady(true);
    else onError(`Voice clone failed: ${await resp.text()}`);
  } catch (e) {
    onError(`Voice clone failed: ${e}`);
  }
}
