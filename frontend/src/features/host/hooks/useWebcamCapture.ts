import { useRef, useCallback } from "react";
import { FRAME_INTERVAL_MS, JPEG_QUALITY_HD, JPEG_QUALITY_4K, RESOLUTION_4K_WIDTH } from "../constants";

type WebcamDeps = {
  isSocketOpen: () => boolean;
  sendJson: (msg: object) => void;
};

export function useWebcamCapture({ isSocketOpen, sendJson }: WebcamDeps) {
  const streamRef = useRef<MediaStream | null>(null);
  const videoElRef = useRef<HTMLVideoElement | null>(null);
  const frameIntervalRef = useRef<ReturnType<typeof setInterval> | null>(null);

  const videoRef = useCallback((el: HTMLVideoElement | null) => {
    videoElRef.current = el;
    if (el && streamRef.current) el.srcObject = streamRef.current;
  }, []);

  const startWebcam = useCallback(async () => {
    try {
      const stream = await navigator.mediaDevices.getUserMedia({
        video: { width: { ideal: 3840 }, height: { ideal: 2160 }, facingMode: "user" },
      });
      streamRef.current = stream;
      if (videoElRef.current) videoElRef.current.srcObject = stream;
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
    if (!video || !isSocketOpen() || !video.videoWidth) return;

    const canvas = document.createElement("canvas");
    canvas.width = video.videoWidth;
    canvas.height = video.videoHeight;
    canvas.getContext("2d")!.drawImage(video, 0, 0, canvas.width, canvas.height);

    const quality = canvas.width > RESOLUTION_4K_WIDTH ? JPEG_QUALITY_4K : JPEG_QUALITY_HD;
    const base64 = canvas.toDataURL("image/jpeg", quality).split(",")[1];
    sendJson({ type: "face:frame", data: base64 });
  }, [isSocketOpen, sendJson]);

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

  return { videoRef, startWebcam, stopWebcam, startFrameStreaming, stopFrameStreaming };
}
