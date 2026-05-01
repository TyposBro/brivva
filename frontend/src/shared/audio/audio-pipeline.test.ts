import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { AudioPipeline, encodeTimestampedPcmFrame } from "./audio-pipeline";

type ProcessorStub = {
  onaudioprocess: ((e: { inputBuffer: { getChannelData: (ch: number) => Float32Array } }) => void) | null;
  connect: ReturnType<typeof vi.fn>;
  disconnect: ReturnType<typeof vi.fn>;
};

function installMocks() {
  const track = { stop: vi.fn() };
  const stream = { getTracks: () => [track] } as unknown as MediaStream;
  const processor: ProcessorStub = {
    onaudioprocess: null,
    connect: vi.fn(),
    disconnect: vi.fn(),
  };
  const source = { connect: vi.fn() };
  const analyser = { fftSize: 0 };
  const ctx = {
    destination: {},
    createMediaStreamSource: vi.fn(() => source),
    createAnalyser: vi.fn(() => analyser),
    createScriptProcessor: vi.fn(() => processor),
    close: vi.fn(),
  };

  const AudioCtxCtor = vi.fn(function (this: unknown) {
    return ctx;
  }) as unknown as { new (arg: unknown): unknown };
  globalThis.AudioContext = AudioCtxCtor as unknown as typeof AudioContext;
  Object.defineProperty(globalThis.navigator, "mediaDevices", {
    configurable: true,
    value: { getUserMedia: vi.fn(async () => stream) },
  });

  return { track, stream, processor, source, analyser, ctx };
}

describe("AudioPipeline", () => {
  let mocks: ReturnType<typeof installMocks>;

  beforeEach(() => {
    mocks = installMocks();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("start returns analyser w/ fftSize=512 (happy)", async () => {
    const pipe = new AudioPipeline();
    const a = await pipe.start(() => {});
    expect(a).toBe(mocks.analyser);
    expect(mocks.analyser.fftSize).toBe(512);
  });

  it("start uses 44100 sampleRate", async () => {
    const pipe = new AudioPipeline();
    await pipe.start(() => {});
    const spy = globalThis.AudioContext as unknown as ReturnType<typeof vi.fn>;
    expect(spy.mock.calls[0][0]).toEqual({ sampleRate: 44100 });
  });

  it("start wires source → analyser AND source → processor", async () => {
    const pipe = new AudioPipeline();
    await pipe.start(() => {});
    expect(mocks.source.connect).toHaveBeenCalledWith(mocks.analyser);
    expect(mocks.source.connect).toHaveBeenCalledWith(mocks.processor);
    expect(mocks.processor.connect).toHaveBeenCalledWith(mocks.ctx.destination);
  });

  it("onaudioprocess emits raw Int16 PCM ArrayBuffer by default", async () => {
    const pipe = new AudioPipeline();
    const onAudio = vi.fn();
    await pipe.start(onAudio);

    const float32 = new Float32Array([0, 0.5, -0.5, 1, -1, 2, -2]);
    mocks.processor.onaudioprocess!({
      inputBuffer: { getChannelData: () => float32 },
    });

    expect(onAudio).toHaveBeenCalledOnce();
    const buf = onAudio.mock.calls[0][0] as ArrayBuffer;
    const int16 = new Int16Array(buf);
    expect(int16[0]).toBe(0);
    expect(int16[1]).toBe(16384);
    expect(int16[2]).toBe(-16384);
    // clipping sanity
    expect(int16[5]).toBe(32767);
    expect(int16[6]).toBe(-32768);
  });

  it("encodes timestamped PCM frames when enabled", async () => {
    vi.spyOn(performance, "now").mockReturnValue(123.456);
    const pipe = new AudioPipeline();
    const onAudio = vi.fn();
    await pipe.start(onAudio, { timestampedAudio: true });

    mocks.processor.onaudioprocess!({
      inputBuffer: { getChannelData: () => new Float32Array([0.5, -0.5]) },
    });

    const buf = onAudio.mock.calls[0][0] as ArrayBuffer;
    const view = new DataView(buf);
    expect(String.fromCharCode(...new Uint8Array(buf, 0, 4))).toBe("BTA2");
    expect(view.getUint8(4)).toBe(1);
    expect(view.getUint32(8, true)).toBe(0);
    expect(view.getUint32(12, true)).toBe(0);
    expect(view.getUint32(16, true)).toBe(44100);
    expect(view.getUint32(20, true)).toBe(123456);
    expect(Array.from(new Int16Array(buf.slice(28)))).toEqual([16384, -16384]);
  });

  it("increments timestamped PCM sample_index per emitted chunk", async () => {
    const pipe = new AudioPipeline();
    const onAudio = vi.fn();
    await pipe.start(onAudio, { timestampedAudio: true });

    for (const sample of [new Float32Array([0, 0, 0]), new Float32Array([0, 0])]) {
      mocks.processor.onaudioprocess!({ inputBuffer: { getChannelData: () => sample } });
    }

    const second = onAudio.mock.calls[1][0] as ArrayBuffer;
    expect(new DataView(second).getUint32(8, true)).toBe(3);
  });

  it("standalone timestamped frame encoder writes little-endian header", () => {
    const pcm = new Int16Array([1, -2]).buffer;
    const encoded = encodeTimestampedPcmFrame({
      pcm,
      sampleIndex: 88_200,
      sampleRate: 44_100,
      clientCaptureTimeUs: 55_000,
    });
    const view = new DataView(encoded);

    expect(String.fromCharCode(...new Uint8Array(encoded, 0, 4))).toBe("BTA2");
    expect(view.getUint8(4)).toBe(1);
    expect(view.getUint32(8, true)).toBe(88_200);
    expect(view.getUint32(16, true)).toBe(44_100);
    expect(view.getUint32(20, true)).toBe(55_000);
    expect(Array.from(new Int16Array(encoded.slice(28)))).toEqual([1, -2]);
  });

  it("stop disconnects processor, stops tracks, closes ctx", async () => {
    const pipe = new AudioPipeline();
    await pipe.start(() => {});
    pipe.stop();
    expect(mocks.processor.disconnect).toHaveBeenCalledOnce();
    expect(mocks.track.stop).toHaveBeenCalledOnce();
    expect(mocks.ctx.close).toHaveBeenCalledOnce();
  });

  it("stop before start is safe (sad: user clicks stop twice)", () => {
    const pipe = new AudioPipeline();
    expect(() => pipe.stop()).not.toThrow();
  });

  it("getUserMedia rejection propagates (sad: mic denied)", async () => {
    (navigator.mediaDevices.getUserMedia as ReturnType<typeof vi.fn>).mockRejectedValueOnce(
      new DOMException("denied", "NotAllowedError"),
    );
    const pipe = new AudioPipeline();
    await expect(pipe.start(() => {})).rejects.toThrow("denied");
  });
});
