const SAMPLE_RATE = 44100;
const BUFFER_SIZE = 4096;
const WORKLET_FRAME_SIZE = 882; // 20 ms @ 44.1 kHz
const FFT_SIZE = 512;
const TIMESTAMPED_AUDIO_HEADER_BYTES = 28;
const TIMESTAMPED_AUDIO_MAGIC = "BTA2";
const TIMESTAMPED_AUDIO_VERSION = 1;

const HIGH_FIDELITY_MIC_CONSTRAINTS: MediaTrackConstraints = {
  channelCount: { ideal: 1 },
  sampleRate: { ideal: SAMPLE_RATE },
  sampleSize: { ideal: 16 },
  // These browser call-processing filters are useful for meetings, but they
  // smear/duck speech badly when we rebroadcast the host voice. Capture clean
  // PCM and let STT/TTS/RTMP handle the downstream processing.
  echoCancellation: false,
  noiseSuppression: false,
  autoGainControl: false,
};

export type AudioPipelineStartOptions = {
  timestampedAudio?: boolean;
};

const PCM_WORKLET_SOURCE = `
class BrivvaPcmWorklet extends AudioWorkletProcessor {
  constructor() {
    super();
    this.frameSize = ${WORKLET_FRAME_SIZE};
    this.buffer = new Float32Array(this.frameSize);
    this.offset = 0;
  }

  process(inputs, outputs) {
    const input = inputs[0]?.[0];
    const output = outputs[0]?.[0];
    if (output) output.fill(0);
    if (!input) return true;

    let read = 0;
    while (read < input.length) {
      const n = Math.min(this.frameSize - this.offset, input.length - read);
      this.buffer.set(input.subarray(read, read + n), this.offset);
      this.offset += n;
      read += n;

      if (this.offset === this.frameSize) {
        const out = new Int16Array(this.frameSize);
        for (let i = 0; i < this.frameSize; i++) {
          out[i] = Math.max(-32768, Math.min(32767, this.buffer[i] * 32768));
        }
        this.port.postMessage(out.buffer, [out.buffer]);
        this.offset = 0;
      }
    }
    return true;
  }
}
registerProcessor("brivva-pcm-worklet", BrivvaPcmWorklet);
`;

export class AudioPipeline {
  private ctx: AudioContext | null = null;
  private processor: ScriptProcessorNode | null = null;
  private workletNode: AudioWorkletNode | null = null;
  private workletSink: GainNode | null = null;
  private workletUrl: string | null = null;
  private stream: MediaStream | null = null;
  private nextSampleIndex = 0;

  async start(
    onAudio: (buffer: ArrayBuffer) => void,
    options: AudioPipelineStartOptions = {},
  ): Promise<AnalyserNode> {
    this.nextSampleIndex = 0;
    this.stream = await this.captureMic();
    this.ctx = new AudioContext({ sampleRate: SAMPLE_RATE });
    const source = this.ctx.createMediaStreamSource(this.stream);

    const analyser = this.createAnalyser(source);
    await this.startStreaming(source, onAudio, options.timestampedAudio === true);

    return analyser;
  }

  private captureMic(): Promise<MediaStream> {
    return navigator.mediaDevices.getUserMedia({
      audio: HIGH_FIDELITY_MIC_CONSTRAINTS,
    });
  }

  private createAnalyser(source: MediaStreamAudioSourceNode): AnalyserNode {
    const analyser = this.ctx!.createAnalyser();
    analyser.fftSize = FFT_SIZE;
    source.connect(analyser);
    return analyser;
  }

  private async startStreaming(
    source: MediaStreamAudioSourceNode,
    onAudio: (buffer: ArrayBuffer) => void,
    timestampedAudio: boolean,
  ): Promise<void> {
    if (await this.tryStartAudioWorklet(source, onAudio, timestampedAudio)) return;
    this.startScriptProcessorFallback(source, onAudio, timestampedAudio);
  }

  private async tryStartAudioWorklet(
    source: MediaStreamAudioSourceNode,
    onAudio: (buffer: ArrayBuffer) => void,
    timestampedAudio: boolean,
  ): Promise<boolean> {
    if (!this.ctx?.audioWorklet || typeof AudioWorkletNode === "undefined") {
      return false;
    }

    try {
      const blob = new Blob([PCM_WORKLET_SOURCE], { type: "text/javascript" });
      this.workletUrl = URL.createObjectURL(blob);
      await this.ctx.audioWorklet.addModule(this.workletUrl);

      const node = new AudioWorkletNode(this.ctx, "brivva-pcm-worklet", {
        numberOfInputs: 1,
        numberOfOutputs: 1,
        outputChannelCount: [1],
      });
      node.port.onmessage = (event: MessageEvent<ArrayBuffer>) => {
        this.emitAudio(event.data, onAudio, timestampedAudio);
      };

      // Keep the worklet in the live audio graph without playing mic audio
      // back to the host. Some browsers suspend unconnected processing nodes.
      const sink = this.ctx.createGain();
      sink.gain.value = 0;
      source.connect(node);
      node.connect(sink);
      sink.connect(this.ctx.destination);

      this.workletNode = node;
      this.workletSink = sink;
      return true;
    } catch (err) {
      console.warn("AudioWorklet unavailable, falling back to ScriptProcessor", err);
      this.cleanupWorklet();
      return false;
    }
  }

  private startScriptProcessorFallback(
    source: MediaStreamAudioSourceNode,
    onAudio: (buffer: ArrayBuffer) => void,
    timestampedAudio: boolean,
  ): void {
    this.processor = this.ctx!.createScriptProcessor(BUFFER_SIZE, 1, 1);
    this.processor.onaudioprocess = (e) =>
      this.emitAudio(
        toInt16(e.inputBuffer.getChannelData(0)),
        onAudio,
        timestampedAudio,
      );
    source.connect(this.processor);
    this.processor.connect(this.ctx!.destination);
  }

  stop(): void {
    this.processor?.disconnect();
    this.cleanupWorklet();
    this.stream?.getTracks().forEach((t) => t.stop());
    this.ctx?.close();
    this.processor = null;
    this.stream = null;
    this.ctx = null;
    this.nextSampleIndex = 0;
  }

  private emitAudio(
    pcm: ArrayBuffer,
    onAudio: (buffer: ArrayBuffer) => void,
    timestampedAudio: boolean,
  ): void {
    if (!timestampedAudio) {
      onAudio(pcm);
      return;
    }
    const sampleIndex = this.nextSampleIndex;
    this.nextSampleIndex += pcm.byteLength / Int16Array.BYTES_PER_ELEMENT;
    onAudio(
      encodeTimestampedPcmFrame({
        pcm,
        sampleIndex,
        sampleRate: SAMPLE_RATE,
        clientCaptureTimeUs: Math.round(performance.now() * 1000),
      }),
    );
  }

  private cleanupWorklet(): void {
    this.workletNode?.disconnect();
    this.workletNode?.port.close?.();
    this.workletSink?.disconnect();
    if (this.workletUrl) URL.revokeObjectURL(this.workletUrl);
    this.workletNode = null;
    this.workletSink = null;
    this.workletUrl = null;
  }
}

export function encodeTimestampedPcmFrame(args: {
  pcm: ArrayBuffer;
  sampleIndex: number;
  sampleRate: number;
  clientCaptureTimeUs: number;
}): ArrayBuffer {
  const out = new ArrayBuffer(
    TIMESTAMPED_AUDIO_HEADER_BYTES + args.pcm.byteLength,
  );
  const bytes = new Uint8Array(out);
  for (let i = 0; i < TIMESTAMPED_AUDIO_MAGIC.length; i++) {
    bytes[i] = TIMESTAMPED_AUDIO_MAGIC.charCodeAt(i);
  }
  const view = new DataView(out);
  view.setUint8(4, TIMESTAMPED_AUDIO_VERSION);
  setU64(view, 8, args.sampleIndex);
  view.setUint32(16, args.sampleRate, true);
  setU64(view, 20, args.clientCaptureTimeUs);
  bytes.set(new Uint8Array(args.pcm), TIMESTAMPED_AUDIO_HEADER_BYTES);
  return out;
}

function setU64(view: DataView, offset: number, value: number): void {
  const safe = Math.max(0, Math.floor(value));
  view.setUint32(offset, safe >>> 0, true);
  view.setUint32(offset + 4, Math.floor(safe / 2 ** 32), true);
}

function toInt16(float32: Float32Array): ArrayBuffer {
  const out = new Int16Array(float32.length);
  for (let i = 0; i < float32.length; i++) {
    out[i] = Math.max(-32768, Math.min(32767, float32[i] * 32768));
  }
  return out.buffer;
}
