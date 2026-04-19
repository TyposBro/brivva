import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { AudioPipeline } from "./audio-pipeline";

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

  it("onaudioprocess emits Int16 PCM ArrayBuffer", async () => {
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
