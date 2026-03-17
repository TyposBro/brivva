import { useReducer, useRef, useCallback } from "react";
import { AudioPipeline } from "../lib/AudioPipeline";
import { RoomSocket } from "../lib/RoomSocket";
import { useTimings, type UtteranceTiming } from "./useTimings";
import { hostReducer, INITIAL_STATE } from "../state/host/reducer";
import { createMessageHandler } from "../state/host/messageHandler";

export type { UtteranceTiming };
export type { HostStatus, GuestCounts, HostUtterance } from "../state/host/reducer";

export function useHostRoom() {
  const [state, dispatch] = useReducer(hostReducer, INITIAL_STATE);
  const { timings, startTimer, recordSplit, finalize, reset: resetTimings } = useTimings();

  const audio = useRef(new AudioPipeline());
  const socket = useRef(new RoomSocket());
  const videoRef = useRef<HTMLVideoElement | null>(null);
  const streamRef = useRef<MediaStream | null>(null);

  const handleMessage = createMessageHandler(
    dispatch,
    () => state.guestCounts,
    { startTimer, recordSplit, finalize },
  );

  // --- webcam ---

  const startWebcam = useCallback(async () => {
    try {
      const stream = await navigator.mediaDevices.getUserMedia({
        video: { width: 512, height: 512, facingMode: "user" },
      });
      streamRef.current = stream;
      if (videoRef.current) {
        videoRef.current.srcObject = stream;
      }
    } catch (err) {
      console.error("Webcam access failed:", err);
    }
  }, []);

  const stopWebcam = useCallback(() => {
    streamRef.current?.getTracks().forEach((t) => t.stop());
    streamRef.current = null;
  }, []);

  const captureFace = useCallback(() => {
    const video = videoRef.current;
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

    socket.current.sendJson({ type: "face:image", data: base64 });
    console.log("[HOST] sent face image for avatar preparation");
  }, []);

  // --- actions ---

  const stopRecording = () => {
    audio.current.stop();
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
          // Capture face when room is created
          if (msg.type === "room:created") {
            // Short delay to ensure webcam is streaming
            setTimeout(captureFace, 500);
          }
        },
        onClose: () => { stopRecording(); dispatch({ type: "disconnected" }); },
      },
    );
  };

  const startRecording = async () => {
    if (!socket.current.isOpen) return;
    const analyser = await audio.current.start((buf) => socket.current.sendAudio(buf));
    dispatch({ type: "recording_started", analyser });
  };

  const closeRoom = () => {
    socket.current.sendJson({ type: "host:end" });
    socket.current.close();
    stopRecording();
    stopWebcam();
  };

  return {
    ...state, timings, videoRef,
    createRoom, startRecording, stopRecording, closeRoom,
  };
}
