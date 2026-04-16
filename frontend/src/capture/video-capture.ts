import { SessionClock } from "./session-clock";

export type VideoChunkHandler = (chunk: {
  captureTsMs: bigint;
  durationMs: number;
  isKeyframe: boolean;
  chunkKind: 0 | 1;
  payload: ArrayBuffer;
}) => void;

export class VideoCapture {
  private recorder: MediaRecorder | null = null;
  private stream: MediaStream | null = null;
  private sentInit = false;

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

    this.recorder = new MediaRecorder(this.stream, {
      mimeType: "video/webm;codecs=vp8",
      videoBitsPerSecond: 4_000_000,
    });
    this.recorder.ondataavailable = async (event) => {
      const payload = await event.data.arrayBuffer();
      onChunk({
        captureTsMs: this.clock.nowMs(),
        durationMs: 33,
        isKeyframe: !this.sentInit,
        chunkKind: this.sentInit ? 1 : 0,
        payload,
      });
      this.sentInit = true;
    };
    this.recorder.start(250);
  }

  stop(): void {
    this.recorder?.stop();
    this.stream?.getTracks().forEach((track) => track.stop());
    this.recorder = null;
    this.stream = null;
    this.sentInit = false;
  }
}
