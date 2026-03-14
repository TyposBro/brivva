const SAMPLE_RATE = 16000;
const BUFFER_SIZE = 4096;
const FFT_SIZE = 512;

export class AudioPipeline {
  private ctx: AudioContext | null = null;
  private processor: ScriptProcessorNode | null = null;
  private stream: MediaStream | null = null;

  async start(onAudio: (buffer: ArrayBuffer) => void): Promise<AnalyserNode> {
    this.stream = await this.captureMic();
    this.ctx = new AudioContext({ sampleRate: SAMPLE_RATE });
    const source = this.ctx.createMediaStreamSource(this.stream);

    const analyser = this.createAnalyser(source);
    this.startStreaming(source, onAudio);

    return analyser;
  }

  private captureMic(): Promise<MediaStream> {
    return navigator.mediaDevices.getUserMedia({ audio: true });
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
      onAudio(toInt16(e.inputBuffer.getChannelData(0)));
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

function toInt16(float32: Float32Array): ArrayBuffer {
  const out = new Int16Array(float32.length);
  for (let i = 0; i < float32.length; i++) {
    out[i] = Math.max(-32768, Math.min(32767, float32[i] * 32768));
  }
  return out.buffer;
}
