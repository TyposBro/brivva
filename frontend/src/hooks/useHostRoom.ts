import { useReducer, useRef, useCallback } from "react";
import { AudioPipeline } from "../lib/AudioPipeline";
import { RoomSocket } from "../lib/RoomSocket";
import { useTimings, type UtteranceTiming } from "./useTimings";
import { hostReducer, INITIAL_STATE } from "../state/host/reducer";
import { createMessageHandler } from "../state/host/messageHandler";

export type { UtteranceTiming };
export type { HostStatus, GuestCounts, HostUtterance } from "../state/host/reducer";

const VOICE_SAMPLE_SECONDS = 30;
const VOICE_SAMPLE_RATE = 44100;

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
        video: { width: 512, height: 512, facingMode: "user" },
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
    if (!video || !socket.current.isOpen) return;

    const canvas = document.createElement("canvas");
    canvas.width = 256;
    canvas.height = 256;
    const ctx = canvas.getContext("2d")!;

    // Center crop to square, then scale to 256x256
    const size = Math.min(video.videoWidth, video.videoHeight);
    const sx = (video.videoWidth - size) / 2;
    const sy = (video.videoHeight - size) / 2;
    ctx.drawImage(video, sx, sy, size, size, 0, 0, 256, 256);

    // Extract base64 (strip data:image/jpeg;base64, prefix)
    const dataUrl = canvas.toDataURL("image/jpeg", 0.9);
    const base64 = dataUrl.split(",")[1];

    socket.current.sendJson({ type: "face:frame", data: base64 });
  }, []);

  const startFrameStreaming = useCallback(() => {
    stopFrameStreaming();
    frameIntervalRef.current = setInterval(captureAndSendFrame, 33); // ~30fps
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
      const processor = ctx.createScriptProcessor(4096, 1, 1);
      processor.onaudioprocess = (e) => {
        const float32 = e.inputBuffer.getChannelData(0);
        const int16 = new Int16Array(float32.length);
        for (let i = 0; i < float32.length; i++) {
          int16[i] = Math.max(-32768, Math.min(32767, Math.round(float32[i] * 32768)));
        }
        voicePcmRef.current.push(int16);
      };
      source.connect(processor);
      processor.connect(ctx.destination);
      voiceRecorderRef.current = { ctx, source, processor };

      // Auto-stop after VOICE_SAMPLE_SECONDS
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

    // Merge PCM chunks
    const totalLen = voicePcmRef.current.reduce((s, c) => s + c.length, 0);
    const merged = new Int16Array(totalLen);
    let offset = 0;
    for (const chunk of voicePcmRef.current) {
      merged.set(chunk, offset);
      offset += chunk.length;
    }
    voicePcmRef.current = [];

    // Convert to base64 and send to server
    const bytes = new Uint8Array(merged.buffer);
    let binary = "";
    for (let i = 0; i < bytes.length; i++) {
      binary += String.fromCharCode(bytes[i]);
    }
    const b64 = btoa(binary);

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

  const createRoom = () => {
    dispatch({ type: "reset" });
    resetTimings();
    startWebcam();
    socket.current.connect(
      { role: "host", sourceLang: "en" },
      {
        onMessage: (msg) => {
          handleMessage(msg);
        },
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
