import {
  AUDIO_BYTES_PER_FRAME,
  AUDIO_FRAME_DURATION_MS,
  AUDIO_SAMPLES_PER_FRAME,
  SAMPLE_RATE,
  float32ToInt16Pcm,
} from "../core/audio/pcm";
import { SessionClock } from "./session-clock";

export type AudioFrameHandler = (frame: {
  captureTsMs: bigint;
  durationMs: number;
  payload: ArrayBuffer;
}) => void;

export class AudioCapture {
  private ctx: AudioContext | null = null;
  private processor: ScriptProcessorNode | null = null;
  private stream: MediaStream | null = null;
  private pending = new Float32Array(0);

  constructor(private readonly clock: SessionClock) {}

  async start(onFrame: AudioFrameHandler, deviceId?: string): Promise<void> {
    this.stream = await navigator.mediaDevices.getUserMedia({
      audio: deviceId ? { deviceId: { exact: deviceId } } : true,
      video: false,
    });

    this.ctx = new AudioContext({ sampleRate: SAMPLE_RATE });
    const source = this.ctx.createMediaStreamSource(this.stream);
    this.processor = this.ctx.createScriptProcessor(1024, 1, 1);
    this.processor.onaudioprocess = (event) => {
      const input = event.inputBuffer.getChannelData(0);
      this.pushSamples(input, onFrame);
    };

    source.connect(this.processor);
    this.processor.connect(this.ctx.destination);
  }

  stop(): void {
    this.processor?.disconnect();
    this.stream?.getTracks().forEach((track) => track.stop());
    void this.ctx?.close();
    this.processor = null;
    this.stream = null;
    this.ctx = null;
    this.pending = new Float32Array(0);
  }

  private pushSamples(input: Float32Array, onFrame: AudioFrameHandler): void {
    const combined = new Float32Array(this.pending.length + input.length);
    combined.set(this.pending, 0);
    combined.set(input, this.pending.length);

    let offset = 0;
    while (combined.length - offset >= AUDIO_SAMPLES_PER_FRAME) {
      const slice = combined.subarray(offset, offset + AUDIO_SAMPLES_PER_FRAME);
      const pcm = float32ToInt16Pcm(slice);
      const payload = pcm.buffer.slice(0, AUDIO_BYTES_PER_FRAME);
      onFrame({
        captureTsMs: this.clock.nowMs(),
        durationMs: AUDIO_FRAME_DURATION_MS,
        payload: payload as ArrayBuffer,
      });
      offset += AUDIO_SAMPLES_PER_FRAME;
    }

    this.pending = combined.slice(offset);
  }
}
