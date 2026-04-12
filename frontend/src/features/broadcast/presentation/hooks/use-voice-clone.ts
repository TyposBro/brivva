import { useState, useEffect, useCallback, useRef } from "react";
import { appConfig } from "../../../../orchestration/config/app-config";
import { float32ToInt16, mergePcmChunks } from "../../../../core/audio/pcm";
import {
  CLONE_MIN_DURATION_SEC,
  CLONE_MAX_DURATION_SEC,
  CLONE_SAMPLE_RATE,
  CLONE_BUFFER_SIZE,
  CLONE_ELAPSED_INTERVAL_MS,
} from "../../domain/broadcast-constants";
import { checkVoiceStatus as fetchVoiceStatus, uploadVoiceClone } from "../../data/voice-clone-api-client";

export type ClonePhase = "idle" | "recording" | "uploading";

export function useVoiceClone(onError: (msg: string) => void, provider = "elevenlabs") {
  const [phase, setPhase] = useState<ClonePhase>("idle");
  const [elapsedSec, setElapsedSec] = useState(0);
  const [voiceReady, setVoiceReady] = useState(false);

  const stopRef = useRef<(() => void) | null>(null);

  const checkVoiceStatus = useCallback(() => {
    fetchVoiceStatus(appConfig.apiBaseUrl)
      .then((isActive) => { if (isActive) setVoiceReady(true); })
      .catch(() => {});
  }, []);

  useEffect(() => { checkVoiceStatus(); }, [checkVoiceStatus]);

  const cloneVoice = useCallback(async () => {
    setPhase("recording");
    setElapsedSec(0);
    setVoiceReady(false);

    const { chunks, cleanup } = await recordAudio();
    const timer = startElapsedTimer(setElapsedSec);

    const pcm = await waitForStopOrMax(chunks, cleanup, timer, stopRef);

    setPhase("uploading");
    try {
      await uploadVoiceClone(appConfig.apiBaseUrl, pcm, provider);
      setVoiceReady(true);
    } catch (e) {
      onError(`${e}`);
    }
    setPhase("idle");
  }, [onError, provider]);

  const stopCloning = useCallback(() => {
    stopRef.current?.();
  }, []);

  const isMinReached = elapsedSec >= CLONE_MIN_DURATION_SEC;

  return { phase, elapsedSec, isMinReached, voiceReady, setVoiceReady, cloneVoice, stopCloning };
}

function waitForStopOrMax(
  chunks: Int16Array[],
  cleanup: () => void,
  timer: number,
  stopRef: React.MutableRefObject<(() => void) | null>,
): Promise<Int16Array> {
  return new Promise((resolve) => {
    let settled = false;

    const finish = () => {
      if (settled) return;
      settled = true;
      clearInterval(timer);
      cleanup();
      stopRef.current = null;
      resolve(mergePcmChunks(chunks));
    };

    stopRef.current = finish;

    setTimeout(finish, CLONE_MAX_DURATION_SEC * 1000);
  });
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

function startElapsedTimer(setElapsed: (s: number) => void) {
  const start = Date.now();
  return window.setInterval(() => {
    setElapsed(Math.round((Date.now() - start) / 1000));
  }, CLONE_ELAPSED_INTERVAL_MS);
}
