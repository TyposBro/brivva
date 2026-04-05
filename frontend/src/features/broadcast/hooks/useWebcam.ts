import { useRef, useCallback } from "react";
import {
  VIDEO_WIDTH, VIDEO_HEIGHT, VIDEO_FPS,
  VIDEO_BITRATE, RECORDING_CHUNK_MS, VIDEO_TAG,
} from "../constants";

export function useWebcam(videoRef: React.RefObject<HTMLVideoElement | null>) {
  const recorderRef = useRef<MediaRecorder | null>(null);

  const startWebcam = useCallback(async (ws: WebSocket, videoDeviceId?: string) => {
    try {
      const stream = await captureVideo(videoDeviceId);
      attachPreview(videoRef, stream);
      const mimeType = negotiateCodec();
      notifyBackend(ws, mimeType);
      recorderRef.current = createRecorder({ stream, mimeType, ws });
      recorderRef.current.start(RECORDING_CHUNK_MS);
    } catch (e) {
      console.error("[WEBCAM] Failed to start:", e);
    }
  }, [videoRef]);

  const stopWebcam = useCallback(() => {
    stopRecorder(recorderRef);
    releasePreview(videoRef);
  }, [videoRef]);

  return { startWebcam, stopWebcam };
}

function captureVideo(deviceId?: string) {
  const constraints: MediaTrackConstraints = {
    width: { ideal: VIDEO_WIDTH },
    height: { ideal: VIDEO_HEIGHT },
    frameRate: { ideal: VIDEO_FPS },
    ...(deviceId ? { deviceId: { exact: deviceId } } : {}),
  };
  return navigator.mediaDevices.getUserMedia({ video: constraints });
}

function attachPreview(ref: React.RefObject<HTMLVideoElement | null>, stream: MediaStream) {
  const video = ref.current!;
  video.srcObject = stream;
  video.play();
}

function negotiateCodec(): string {
  if (MediaRecorder.isTypeSupported("video/mp4;codecs=avc1.42E01E")) return "video/mp4;codecs=avc1.42E01E";
  if (MediaRecorder.isTypeSupported("video/webm;codecs=h264")) return "video/webm;codecs=h264";
  return "video/webm;codecs=vp8";
}

function notifyBackend(ws: WebSocket, mimeType: string) {
  const isH264 = mimeType.includes("avc1") || mimeType.includes("h264");
  console.log("[WEBCAM] MediaRecorder codec:", mimeType, isH264 ? "(H.264 \u2014 passthrough)" : "(re-encode)");
  if (ws.readyState === WebSocket.OPEN) {
    ws.send(JSON.stringify({ type: "video:codec", codec: isH264 ? "h264" : "vp8", mimeType }));
  }
}

type RecorderConfig = {
  stream: MediaStream;
  mimeType: string;
  ws: WebSocket;
};

function createRecorder({ stream, mimeType, ws }: RecorderConfig): MediaRecorder {
  const recorder = new MediaRecorder(stream, { mimeType, videoBitsPerSecond: VIDEO_BITRATE });
  recorder.ondataavailable = (e) => {
    if (e.data.size > 0 && ws.readyState === WebSocket.OPEN) {
      e.data.arrayBuffer().then((buf) => {
        const tagged = new Uint8Array(buf.byteLength + 1);
        tagged[0] = VIDEO_TAG;
        tagged.set(new Uint8Array(buf), 1);
        ws.send(tagged.buffer);
      });
    }
  };
  return recorder;
}

function stopRecorder(ref: React.MutableRefObject<MediaRecorder | null>) {
  if (ref.current && ref.current.state !== "inactive") ref.current.stop();
  ref.current = null;
}

function releasePreview(ref: React.RefObject<HTMLVideoElement | null>) {
  const video = ref.current;
  if (video?.srcObject) {
    (video.srcObject as MediaStream).getTracks().forEach((t) => t.stop());
    video.srcObject = null;
  }
}
