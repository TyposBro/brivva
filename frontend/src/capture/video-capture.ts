import { SessionClock } from "./session-clock";

export type VideoChunkHandler = (chunk: {
  captureTsMs: bigint;
  durationMs: number;
  isKeyframe: boolean;
  chunkKind: 0 | 1;
  payload: ArrayBuffer;
}) => void;

export class VideoCapture {
  private stream: MediaStream | null = null;
  private canvas: HTMLCanvasElement | null = null;
  private frameTimer: number | null = null;
  private nextCaptureTsMs: bigint | null = null;

  constructor(private readonly clock: SessionClock) {}

  async start(
    previewEl: HTMLVideoElement,
    onChunk: VideoChunkHandler,
    deviceId?: string,
  ): Promise<void> {
    this.stream = await navigator.mediaDevices.getUserMedia({
      video: deviceId ? { deviceId: { exact: deviceId } } : true,
      audio: false,
    });

    previewEl.srcObject = this.stream;
    previewEl.muted = true;
    await previewEl.play();

    this.canvas = document.createElement("canvas");
    this.canvas.width = previewEl.videoWidth || 640;
    this.canvas.height = previewEl.videoHeight || 360;
    this.nextCaptureTsMs = this.clock.nowMs();

    this.frameTimer = window.setInterval(() => {
      void this.captureFrame(previewEl, onChunk);
    }, 33);
  }

  stop(): void {
    if (this.frameTimer !== null) {
      window.clearInterval(this.frameTimer);
    }
    this.stream?.getTracks().forEach((track) => track.stop());
    this.stream = null;
    this.canvas = null;
    this.frameTimer = null;
    this.nextCaptureTsMs = null;
  }

  private async captureFrame(
    previewEl: HTMLVideoElement,
    onChunk: VideoChunkHandler,
  ): Promise<void> {
    if (!this.canvas || previewEl.readyState < HTMLMediaElement.HAVE_CURRENT_DATA) {
      return;
    }

    const context = this.canvas.getContext("2d");
    if (!context) {
      return;
    }

    context.drawImage(previewEl, 0, 0, this.canvas.width, this.canvas.height);
    const blob = await new Promise<Blob | null>((resolve) => {
      this.canvas?.toBlob(resolve, "image/jpeg", 0.8);
    });
    if (!blob || this.nextCaptureTsMs === null) {
      return;
    }

    const payload = await blob.arrayBuffer();
    const captureTsMs = this.nextCaptureTsMs;
    this.nextCaptureTsMs += 33n;
    onChunk({
      captureTsMs,
      durationMs: 33,
      isKeyframe: true,
      chunkKind: 1,
      payload,
    });
  }
}
