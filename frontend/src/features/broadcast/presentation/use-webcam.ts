import { useCallback, useRef } from "react";

const HOST_VIDEO_MAX_WIDTH = 1280;
const HOST_VIDEO_MAX_HEIGHT = 720;
const HOST_CAMERA_IDEAL_WIDTH = 3840;
const HOST_CAMERA_IDEAL_HEIGHT = 2160;
const HOST_VIDEO_FPS = 15;
const HOST_VIDEO_INTERVAL_MS = Math.round(1000 / HOST_VIDEO_FPS);
const HOST_VIDEO_JPEG_QUALITY = 0.72;

/** Webcam capture + JPEG frame streaming — encapsulated so useHostSession stays small. */
export function useWebcam(sendFrameJson: (msg: { type: "face:frame"; data: string }) => void, isSocketOpen: () => boolean) {
  const streamRef = useRef<MediaStream | null>(null);
  const videoElRef = useRef<HTMLVideoElement | null>(null);
  const frameIntervalRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const encodeInFlightRef = useRef(false);

  const videoRef = useCallback((el: HTMLVideoElement | null) => {
    videoElRef.current = el;
    if (el && streamRef.current) {
      el.srcObject = streamRef.current;
    }
  }, []);

  const startWebcam = useCallback(async () => {
    try {
      const stream = await navigator.mediaDevices.getUserMedia({
        video: {
          width: { ideal: HOST_CAMERA_IDEAL_WIDTH, max: HOST_CAMERA_IDEAL_WIDTH },
          height: { ideal: HOST_CAMERA_IDEAL_HEIGHT, max: HOST_CAMERA_IDEAL_HEIGHT },
          frameRate: { ideal: 30, max: 30 },
          facingMode: "user",
        },
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
    if (!video || !isSocketOpen() || !video.videoWidth || encodeInFlightRef.current) return;

    const { width, height } = fitVideoFrame(video.videoWidth, video.videoHeight);
    const canvas = canvasRef.current ?? document.createElement("canvas");
    canvasRef.current = canvas;
    if (canvas.width !== width) canvas.width = width;
    if (canvas.height !== height) canvas.height = height;
    const ctx = canvas.getContext("2d")!;
    ctx.drawImage(video, 0, 0, width, height);

    encodeInFlightRef.current = true;
    canvas.toBlob((blob) => {
      if (!blob) {
        encodeInFlightRef.current = false;
        return;
      }
      const reader = new FileReader();
      reader.onloadend = () => {
        encodeInFlightRef.current = false;
        if (!isSocketOpen() || typeof reader.result !== "string") return;
        const comma = reader.result.indexOf(",");
        if (comma === -1) return;
        sendFrameJson({ type: "face:frame", data: reader.result.slice(comma + 1) });
      };
      reader.onerror = () => {
        encodeInFlightRef.current = false;
      };
      reader.readAsDataURL(blob);
    }, "image/jpeg", HOST_VIDEO_JPEG_QUALITY);
  }, [isSocketOpen, sendFrameJson]);

  const stopFrameStreaming = useCallback(() => {
    if (frameIntervalRef.current !== null) {
      clearInterval(frameIntervalRef.current);
      frameIntervalRef.current = null;
    }
  }, []);

  const startFrameStreaming = useCallback(() => {
    stopFrameStreaming();
    frameIntervalRef.current = setInterval(captureAndSendFrame, HOST_VIDEO_INTERVAL_MS);
  }, [captureAndSendFrame, stopFrameStreaming]);

  const getStream = useCallback(() => streamRef.current, []);

  return { videoRef, startWebcam, stopWebcam, startFrameStreaming, stopFrameStreaming, getStream };
}

function fitVideoFrame(sourceWidth: number, sourceHeight: number): { width: number; height: number } {
  const scale = Math.min(
    1,
    HOST_VIDEO_MAX_WIDTH / sourceWidth,
    HOST_VIDEO_MAX_HEIGHT / sourceHeight,
  );
  return {
    width: Math.max(1, Math.round(sourceWidth * scale)),
    height: Math.max(1, Math.round(sourceHeight * scale)),
  };
}
