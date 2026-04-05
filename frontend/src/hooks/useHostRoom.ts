import { useReducer, useRef, useCallback } from "react";
import { AudioPipeline } from "../lib/AudioPipeline";
import { RoomSocket } from "../lib/RoomSocket";
import { useTimings, type UtteranceTiming } from "./useTimings";
import { hostReducer, INITIAL_STATE } from "../state/host/reducer";
import { createMessageHandler } from "../state/host/messageHandler";
import { float32ToInt16, mergePcmChunks, pcmToBase64 } from "../shared/audio/pcm";

export type { UtteranceTiming };
export type { HostStatus, GuestCounts, HostUtterance } from "../state/host/reducer";

const VOICE_SAMPLE_RATE = 44100;
const VOICE_SAMPLE_SECONDS = 30;
const VOICE_BUFFER_SIZE = 4096;
const FRAME_INTERVAL_MS = 33; // ~30fps
const JPEG_QUALITY_HD = 0.85;
const JPEG_QUALITY_4K = 0.80;
const RESOLUTION_4K_WIDTH = 2000;

export function useHostRoom() {
  const [state, dispatch] = useReducer(hostReducer, INITIAL_STATE);
  const { timings, startTimer, markInterim, recordStt, recordTranslate, recordTts, finalize, reset: resetTimings } = useTimings();

  const audio = useRef(new AudioPipeline());
  const socket = useRef(new RoomSocket());
  const streamRef = useRef<MediaStream | null>(null);
  const videoElRef = useRef<HTMLVideoElement | null>(null);
  const frameIntervalRef = useRef<ReturnType<typeof setInterval> | null>(null);

  // Callback ref: connects stream to video element whenever either becomes available
  const videoRef = useCallback((el: HTMLVideoElement | null) => {
    videoElRef.current = el;
    if (el && streamRef.current) {
      el.srcObject = streamRef.current;
    }
  }, []);

  const handleMessage = createMessageHandler(
    dispatch,
    () => state.guestCounts,
    { startTimer, markInterim, recordStt, recordTranslate, recordTts, finalize },
  );

  // --- webcam ---

  const startWebcam = useCallback(async () => {
    try {
      const stream = await navigator.mediaDevices.getUserMedia({
        video: { width: { ideal: 3840 }, height: { ideal: 2160 }, facingMode: "user" },
      });
      streamRef.current = stream;
      if (videoElRef.current) {
        videoElRef.current.srcObject = stream;
      }
    } catch (err) {
      console.error("Webcam access failed:", err);
    }
  }, []);

  const stopWebcam = useCallback(() => {
    streamRef.current?.getTracks().forEach((t) => t.stop());
    streamRef.current = null;
  }, []);

  const captureAndSendFrame = useCallback(() => {
    const video = videoElRef.current;
    if (!video || !socket.current.isOpen || !video.videoWidth) return;

    const w = video.videoWidth;
    const h = video.videoHeight;

    const canvas = document.createElement("canvas");
    canvas.width = w;
    canvas.height = h;
    canvas.getContext("2d")!.drawImage(video, 0, 0, w, h);

    const quality = w > RESOLUTION_4K_WIDTH ? JPEG_QUALITY_4K : JPEG_QUALITY_HD;
    const base64 = canvas.toDataURL("image/jpeg", quality).split(",")[1];
    socket.current.sendJson({ type: "face:frame", data: base64 });
  }, []);

  const startFrameStreaming = useCallback(() => {
    stopFrameStreaming();
    frameIntervalRef.current = setInterval(captureAndSendFrame, FRAME_INTERVAL_MS);
  }, [captureAndSendFrame]);

  const stopFrameStreaming = useCallback(() => {
    if (frameIntervalRef.current !== null) {
      clearInterval(frameIntervalRef.current);
      frameIntervalRef.current = null;
    }
  }, []);

  // --- voice sample recording ---

  const voicePcmRef = useRef<Int16Array[]>([]);
  const voiceRecorderRef = useRef<{ ctx: AudioContext; source: MediaStreamAudioSourceNode; processor: ScriptProcessorNode } | null>(null);

  const startVoiceRecording = useCallback(async () => {
    voicePcmRef.current = [];
    try {
      const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
      const ctx = new AudioContext({ sampleRate: VOICE_SAMPLE_RATE });
      const source = ctx.createMediaStreamSource(stream);
      const processor = ctx.createScriptProcessor(VOICE_BUFFER_SIZE, 1, 1);
      processor.onaudioprocess = (e) => {
        voicePcmRef.current.push(float32ToInt16(e.inputBuffer.getChannelData(0)));
      };
      source.connect(processor);
      processor.connect(ctx.destination);
      voiceRecorderRef.current = { ctx, source, processor };

      setTimeout(() => stopVoiceRecording(), VOICE_SAMPLE_SECONDS * 1000);
    } catch (err) {
      console.error("Voice recording failed:", err);
    }
  }, []);

  const stopVoiceRecording = useCallback(() => {
    const rec = voiceRecorderRef.current;
    if (!rec) return;
    rec.processor.disconnect();
    rec.source.disconnect();
    rec.ctx.close();
    voiceRecorderRef.current = null;

    const merged = mergePcmChunks(voicePcmRef.current);
    voicePcmRef.current = [];
    const b64 = pcmToBase64(merged);

    dispatch({ type: "voice_cloning" });
    socket.current.sendJson({ type: "voice:sample", data: b64 });
  }, []);

  const skipVoiceSetup = useCallback(() => {
    dispatch({ type: "skip_voice_setup" });
  }, []);

  // --- actions ---

  const stopRecording = () => {
    audio.current.stop();
    stopFrameStreaming();
    dispatch({ type: "recording_stopped" });
  };

  const createRoom = (opts?: { sessionId?: string; sourceLang?: string }) => {
    dispatch({ type: "reset" });
    resetTimings();
    startWebcam();
    const params: Record<string, string> = { role: "host", sourceLang: opts?.sourceLang ?? "en" };
    if (opts?.sessionId) params.sessionId = opts.sessionId;
    socket.current.connect(
      params,
      {
        onMessage: (msg) => { handleMessage(msg); },
        onClose: () => { stopRecording(); dispatch({ type: "disconnected" }); },
      },
    );
  };

  const startRecording = async () => {
    if (!socket.current.isOpen) return;
    const analyser = await audio.current.start((buf) => socket.current.sendAudio(buf));
    dispatch({ type: "recording_started", analyser });
    startFrameStreaming();
  };

  const closeRoom = () => {
    socket.current.sendJson({ type: "host:end" });
    socket.current.close();
    stopRecording();
    stopFrameStreaming();
    stopWebcam();
  };

  return {
    ...state, timings, videoRef,
    createRoom, startRecording, stopRecording, closeRoom,
    startVoiceRecording, stopVoiceRecording, skipVoiceSetup,
  };
}
