import { float32ToInt16 } from "../shared/audio/pcm";

const SAMPLE_RATE = 44100;
const BUFFER_SIZE = 4096;
const FFT_SIZE = 512;

export class AudioPipeline {
  private ctx: AudioContext | null = null;
  private processor: ScriptProcessorNode | null = null;
  private stream: MediaStream | null = null;

  async start(onAudio: (buffer: ArrayBuffer) => void, deviceId?: string): Promise<AnalyserNode> {
    this.stream = await this.captureMic(deviceId);
    this.ctx = new AudioContext({ sampleRate: SAMPLE_RATE });
    const source = this.ctx.createMediaStreamSource(this.stream);

    const analyser = this.createAnalyser(source);
    this.startStreaming(source, onAudio);

    return analyser;
  }

  private captureMic(deviceId?: string): Promise<MediaStream> {
    const constraints: MediaStreamConstraints = {
      audio: deviceId ? { deviceId: { exact: deviceId } } : true,
    };
    return navigator.mediaDevices.getUserMedia(constraints);
  }

  private createAnalyser(source: MediaStreamAudioSourceNode): AnalyserNode {
    const analyser = this.ctx!.createAnalyser();
    analyser.fftSize = FFT_SIZE;
    source.connect(analyser);
    return analyser;
  }

  private startStreaming(
    source: MediaStreamAudioSourceNode,
    onAudio: (buffer: ArrayBuffer) => void
  ): void {
    this.processor = this.ctx!.createScriptProcessor(BUFFER_SIZE, 1, 1);
    this.processor.onaudioprocess = (e) =>
      onAudio(float32ToInt16(e.inputBuffer.getChannelData(0)).buffer as ArrayBuffer);
    source.connect(this.processor);
    this.processor.connect(this.ctx!.destination);
  }

  stop(): void {
    this.processor?.disconnect();
    this.stream?.getTracks().forEach((t) => t.stop());
    this.ctx?.close();
    this.processor = null;
    this.stream = null;
    this.ctx = null;
  }
}
