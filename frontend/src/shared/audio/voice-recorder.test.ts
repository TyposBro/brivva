import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { act, renderHook } from "@testing-library/react";
import { useVoiceRecorder } from "./voice-recorder";

interface FakeProcessor {
  onaudioprocess: ((e: { inputBuffer: { getChannelData: () => Float32Array } }) => void) | null;
  connect: () => void;
  disconnect: () => void;
}

const recorderState = {
  contexts: [] as Array<{ closed: boolean }>,
  processors: [] as FakeProcessor[],
  sources: [] as Array<{ disconnected: boolean }>,
  getUserMedia: vi.fn<() => Promise<MediaStream>>(),
};

function installFakeMedia({ failGetUserMedia = false } = {}) {
  recorderState.contexts = [];
  recorderState.processors = [];
  recorderState.sources = [];
  recorderState.getUserMedia.mockReset();

  if (failGetUserMedia) {
    recorderState.getUserMedia.mockRejectedValue(new Error("Permission denied"));
  } else {
    const stream = { getTracks: () => [{ stop: vi.fn() }] } as unknown as MediaStream;
    recorderState.getUserMedia.mockResolvedValue(stream);
  }

  Object.defineProperty(globalThis.navigator, "mediaDevices", {
    configurable: true,
    value: { getUserMedia: recorderState.getUserMedia },
  });

  class FakeProcessorImpl implements FakeProcessor {
    onaudioprocess: FakeProcessor["onaudioprocess"] = null;
    connect = vi.fn();
    disconnect = vi.fn();
  }

  class FakeSourceImpl {
    disconnected = false;
    connect = vi.fn();
    disconnect = vi.fn(() => {
      this.disconnected = true;
    });
  }

  class FakeAudioContext {
    closed = false;
    destination = {};
    constructor() {
      recorderState.contexts.push(this);
    }
    createMediaStreamSource() {
      const src = new FakeSourceImpl();
      recorderState.sources.push(src);
      return src as unknown as MediaStreamAudioSourceNode;
    }
    createScriptProcessor() {
      const p = new FakeProcessorImpl();
      recorderState.processors.push(p);
      return p as unknown as ScriptProcessorNode;
    }
    close() {
      this.closed = true;
      return Promise.resolve();
    }
  }

  vi.stubGlobal("AudioContext", FakeAudioContext);
}

describe("useVoiceRecorder", () => {
  beforeEach(() => {
    installFakeMedia();
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-04-19T00:00:00Z"));
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  it("start → tick advances elapsedSec; stop returns base64 sample (happy)", async () => {
    const { result } = renderHook(() =>
      useVoiceRecorder({ minSec: 30, maxSec: 180 }),
    );
    await act(async () => {
      await result.current.start();
    });
    expect(result.current.isRecording).toBe(true);

    // Push a chunk into the processor so the merge path produces non-empty bytes.
    act(() => {
      const sample = new Float32Array([0.5, -0.5, 1.0, -1.0]);
      recorderState.processors[0].onaudioprocess!({
        inputBuffer: { getChannelData: () => sample },
      });
    });

    // Two ticks → ~0.5s elapsed; stays under min.
    await act(async () => {
      vi.advanceTimersByTime(500);
    });
    expect(result.current.elapsedSec).toBe(0);

    let stopped: string | null = null;
    act(() => {
      stopped = result.current.stop();
    });
    expect(stopped).not.toBeNull();
    expect(typeof stopped).toBe("string");
    expect(result.current.isRecording).toBe(false);
    expect(recorderState.contexts[0].closed).toBe(true);
  });

  it("auto-stops at maxSec and fires onAutoStop with the encoded sample", async () => {
    const onAutoStop = vi.fn();
    const { result } = renderHook(() =>
      useVoiceRecorder({ minSec: 30, maxSec: 5, onAutoStop }),
    );
    await act(async () => {
      await result.current.start();
    });

    await act(async () => {
      vi.advanceTimersByTime(6_000);
    });

    expect(onAutoStop).toHaveBeenCalledTimes(1);
    expect(typeof onAutoStop.mock.calls[0][0]).toBe("string");
    expect(result.current.isRecording).toBe(false);
  });

  it("stop encodes samples as a RIFF/WAVE container (regression)", async () => {
    const { result } = renderHook(() =>
      useVoiceRecorder({ minSec: 30, maxSec: 180 }),
    );
    await act(async () => {
      await result.current.start();
    });
    act(() => {
      const sample = new Float32Array([0.5, -0.5, 1.0, -1.0]);
      recorderState.processors[0].onaudioprocess!({
        inputBuffer: { getChannelData: () => sample },
      });
    });
    let stopped: string | null = null;
    act(() => {
      stopped = result.current.stop();
    });
    expect(stopped).not.toBeNull();
    const bytes = Uint8Array.from(atob(stopped!), (c) => c.charCodeAt(0));
    const tag = (off: number) =>
      String.fromCharCode(bytes[off]!, bytes[off + 1]!, bytes[off + 2]!, bytes[off + 3]!);
    expect(tag(0)).toBe("RIFF");
    expect(tag(8)).toBe("WAVE");
    expect(tag(12)).toBe("fmt ");
    expect(tag(36)).toBe("data");
  });

  it("stop returns null when never started (sad)", () => {
    const { result } = renderHook(() =>
      useVoiceRecorder({ minSec: 30, maxSec: 180 }),
    );
    let stopped: string | null = "init";
    act(() => {
      stopped = result.current.stop();
    });
    expect(stopped).toBeNull();
  });

  it("start is idempotent — second call while recording is a no-op", async () => {
    const { result } = renderHook(() =>
      useVoiceRecorder({ minSec: 30, maxSec: 180 }),
    );
    await act(async () => {
      await result.current.start();
    });
    await act(async () => {
      await result.current.start();
    });
    expect(recorderState.contexts).toHaveLength(1);
  });

  it("propagates getUserMedia rejection (sad)", async () => {
    installFakeMedia({ failGetUserMedia: true });
    const { result } = renderHook(() =>
      useVoiceRecorder({ minSec: 30, maxSec: 180 }),
    );
    await expect(
      act(async () => {
        await result.current.start();
      }),
    ).rejects.toThrow(/Permission denied/);
    expect(result.current.isRecording).toBe(false);
  });

  it("auto-stop without onAutoStop still tears down cleanly", async () => {
    const { result } = renderHook(() =>
      useVoiceRecorder({ minSec: 30, maxSec: 1 }),
    );
    await act(async () => {
      await result.current.start();
    });
    await act(async () => {
      vi.advanceTimersByTime(1500);
    });
    expect(result.current.isRecording).toBe(false);
    expect(recorderState.contexts[0].closed).toBe(true);
  });
});
