import { describe, it, expect, beforeEach, vi } from "vitest";
import { act, renderHook, waitFor } from "@testing-library/react";

// Mock api
vi.mock("../lib/api", () => ({
  getAuthToken: vi.fn(),
  cloneSessionVoice: vi.fn(),
}));

type Callbacks = {
  onOpen?: () => void;
  onMessage: (m: unknown) => void;
  onBinary?: (b: ArrayBuffer) => void;
  onClose: () => void;
};

type PipeInst = {
  started: boolean;
  stopped: boolean;
  onAudio: ((b: ArrayBuffer) => void) | null;
};
type SockInst = {
  callbacks: Callbacks | null;
  params: Record<string, string> | null;
  sent: unknown[];
  audio: ArrayBuffer[];
  _open: boolean;
  fireOpen: () => void;
  fireMessage: (m: unknown) => void;
  fireClose: () => void;
};

const { pipelineInstances, socketInstances, FakePipeline, FakeSocket } = vi.hoisted(() => {
  const pipelineInstances: PipeInst[] = [];
  const socketInstances: SockInst[] = [];

  class FakePipeline {
    started = false;
    stopped = false;
    onAudio: ((b: ArrayBuffer) => void) | null = null;
    constructor() { pipelineInstances.push(this); }
    async start(onAudio: (b: ArrayBuffer) => void) {
      this.started = true;
      this.onAudio = onAudio;
      return { fftSize: 512 } as unknown as AnalyserNode;
    }
    stop() { this.stopped = true; }
  }

  class FakeSocket {
    callbacks: Callbacks | null = null;
    params: Record<string, string> | null = null;
    sent: unknown[] = [];
    audio: ArrayBuffer[] = [];
    _open = false;
    constructor() { socketInstances.push(this); }
    connect(params: Record<string, string>, cbs: Callbacks) {
      this.params = params;
      this.callbacks = cbs;
    }
    get isOpen() { return this._open; }
    fireOpen() { this._open = true; this.callbacks?.onOpen?.(); }
    fireMessage(m: unknown) { this.callbacks?.onMessage(m); }
    fireClose() { this._open = false; this.callbacks?.onClose(); }
    sendJson(m: unknown) { this.sent.push(m); }
    sendAudio(b: ArrayBuffer) { this.audio.push(b); }
    close() { this._open = false; }
  }

  return { pipelineInstances, socketInstances, FakePipeline, FakeSocket };
});

vi.mock("../lib/AudioPipeline", () => ({ AudioPipeline: FakePipeline }));
vi.mock("../lib/SessionSocket", () => ({ SessionSocket: FakeSocket }));

import * as api from "../lib/api";
import { useHostSession } from "./useHostSession";

const mockedGetAuthToken = api.getAuthToken as unknown as ReturnType<typeof vi.fn>;
const mockedCloneSessionVoice = api.cloneSessionVoice as unknown as ReturnType<typeof vi.fn>;

function installMedia() {
  const track = { stop: vi.fn() };
  const stream = { getTracks: () => [track] } as unknown as MediaStream;
  Object.defineProperty(globalThis.navigator, "mediaDevices", {
    configurable: true,
    value: { getUserMedia: vi.fn(async () => stream) },
  });

  class FakeScriptProcessorNode {
    onaudioprocess: ((event: { inputBuffer: { getChannelData: () => Float32Array } }) => void) | null = null;
    connect() {}
    disconnect() {}
  }

  class FakeMediaStreamSourceNode {
    connect() {}
    disconnect() {}
  }

  class FakeAudioContext {
    destination = {};
    createMediaStreamSource() { return new FakeMediaStreamSourceNode(); }
    createScriptProcessor() { return new FakeScriptProcessorNode(); }
    close() { return Promise.resolve(); }
  }

  Object.defineProperty(globalThis, "AudioContext", {
    configurable: true,
    value: FakeAudioContext,
  });
}

describe("useHostSession", () => {
  beforeEach(() => {
    pipelineInstances.length = 0;
    socketInstances.length = 0;
    mockedGetAuthToken.mockReset();
    mockedCloneSessionVoice.mockReset();
    installMedia();
  });

  it("initial state is idle", () => {
    const { result } = renderHook(() => useHostSession());
    expect(result.current.status).toBe("idle");
    expect(result.current.utterances).toEqual([]);
    expect(result.current.translations).toEqual({});
    expect(result.current.voiceReady).toBe(false);
    expect(result.current.error).toBeNull();
  });

  it("connectSession w/o userId → dispatches error, skips WS connect (sad)", async () => {
    const { result } = renderHook(() => useHostSession());
    await act(async () => {
      await result.current.connectSession({ userId: "" });
    });
    expect(result.current.error).toContain("user_id");
    expect(socketInstances.filter((s) => s.callbacks)).toHaveLength(0);
  });

  it("connectSession happy path → reset, token fetched, WS connected, status→voice_setup on open", async () => {
    mockedGetAuthToken.mockResolvedValueOnce({ token: "tok_1" });
    const { result } = renderHook(() => useHostSession());

    await act(async () => {
      await result.current.connectSession({ userId: "u1", sessionId: "s1", sourceLang: "en" });
    });

    expect(mockedGetAuthToken).toHaveBeenCalledWith("u1");
    const connected = socketInstances.filter((s) => s.callbacks);
    expect(connected).toHaveLength(1);
    expect(connected[0].params).toMatchObject({
      sourceLang: "en", token: "tok_1", sessionId: "s1",
    });
    expect(result.current.status).toBe("creating");

    act(() => socketInstances[0].fireOpen());
    expect(result.current.status).toBe("voice_setup");
  });

  it("auth token fetch failure (sad)", async () => {
    mockedGetAuthToken.mockRejectedValueOnce(new Error("401 unauthorized"));
    const { result } = renderHook(() => useHostSession());

    await act(async () => {
      await result.current.connectSession({ userId: "u1" });
    });
    expect(result.current.error).toContain("Auth token fetch failed");
    expect(result.current.error).toContain("401");
    expect(socketInstances.filter((s) => s.callbacks)).toHaveLength(0);
  });

  it("skipVoiceSetup → ready", async () => {
    mockedGetAuthToken.mockResolvedValueOnce({ token: "t" });
    const { result } = renderHook(() => useHostSession());
    await act(async () => { await result.current.connectSession({ userId: "u1" }); });
    act(() => socketInstances[0].fireOpen());
    act(() => result.current.skipVoiceSetup());
    expect(result.current.status).toBe("ready");
  });

  it("stopVoiceRecording clones through Workers for the active session", async () => {
    mockedGetAuthToken.mockResolvedValueOnce({ token: "t" });
    mockedCloneSessionVoice.mockResolvedValueOnce({
      voice: { id: "v1", user_id: "u1", elevenlabs_voice_id: "el1", name: "n", created_at: 1 },
    });
    const { result } = renderHook(() => useHostSession());

    await act(async () => {
      await result.current.connectSession({ userId: "u1", sessionId: "s1" });
    });
    act(() => socketInstances[0].fireOpen());

    await act(async () => {
      await result.current.startVoiceRecording();
    });
    await act(async () => {
      result.current.stopVoiceRecording();
    });

    await waitFor(() =>
      expect(mockedCloneSessionVoice).toHaveBeenCalledWith(
        "s1",
        expect.objectContaining({ user_id: "u1" }),
      ),
    );
    await waitFor(() => expect(result.current.status).toBe("ready"));
    expect(result.current.voiceReady).toBe(true);
  });

  it("startRecording bails silently if socket not open (sad)", async () => {
    const { result } = renderHook(() => useHostSession());
    await act(async () => { await result.current.startRecording(); });
    expect(result.current.status).toBe("idle");
    expect(pipelineInstances.every((p) => !p.started)).toBe(true);
  });

  it("startRecording when open → pipeline starts, status=recording, audio forwarded", async () => {
    mockedGetAuthToken.mockResolvedValueOnce({ token: "t" });
    const { result } = renderHook(() => useHostSession());
    await act(async () => { await result.current.connectSession({ userId: "u1" }); });
    act(() => socketInstances[0].fireOpen());
    act(() => result.current.skipVoiceSetup());

    await act(async () => { await result.current.startRecording(); });
    expect(result.current.status).toBe("recording");
    expect(result.current.analyser).not.toBeNull();

    const pipe = pipelineInstances[0];
    const buf = new ArrayBuffer(8);
    pipe.onAudio!(buf);
    expect(socketInstances[0].audio).toEqual([buf]);
  });

  it("stopRecording → pipeline stop + status=ready", async () => {
    mockedGetAuthToken.mockResolvedValueOnce({ token: "t" });
    const { result } = renderHook(() => useHostSession());
    await act(async () => { await result.current.connectSession({ userId: "u1" }); });
    act(() => socketInstances[0].fireOpen());
    act(() => result.current.skipVoiceSetup());
    await act(async () => { await result.current.startRecording(); });
    act(() => result.current.stopRecording());
    expect(result.current.status).toBe("ready");
    expect(pipelineInstances[0].stopped).toBe(true);
    expect(result.current.analyser).toBeNull();
  });

  it("full pipeline: interim → final → translation → tts_end populates timings + transcripts", async () => {
    mockedGetAuthToken.mockResolvedValueOnce({ token: "t" });
    const { result } = renderHook(() => useHostSession());
    act(() => result.current.setActiveTargetLangs(["ja"]));
    await act(async () => { await result.current.connectSession({ userId: "u1" }); });
    act(() => socketInstances[0].fireOpen());
    act(() => result.current.skipVoiceSetup());
    await act(async () => { await result.current.startRecording(); });

    const s = socketInstances[0];
    act(() => s.fireMessage({ type: "interim", transcript: "hel" }));
    expect(result.current.liveTranscript).toBe("hel");

    act(() => s.fireMessage({ type: "final", utteranceId: 1, transcript: "hello", sttMs: 200 }));
    expect(result.current.liveTranscript).toBe("");
    expect(result.current.utterances).toEqual([{ id: 1, transcript: "hello" }]);

    act(() => s.fireMessage({ type: "translation", utteranceId: 1, targetLang: "ja", text: "こ", translateMs: 120 }));
    expect(result.current.translations.ja).toEqual({ id: 1, text: "こ" });

    act(() => s.fireMessage({ type: "tts_end", utteranceId: 1, ttsMs: 300 }));
    await waitFor(() => expect(result.current.timings).toHaveLength(1));
    const t = result.current.timings[0];
    expect(t.id).toBe("1");
    expect(t.sttMs).toBe(200);
    expect(t.translateMs).toBe(120);
    expect(t.ttsMs).toBe(300);
    expect(t.langs).toEqual(["ja"]);
  });

  it("WS disconnect mid-recording → status=disconnected, pipeline stopped (sad)", async () => {
    mockedGetAuthToken.mockResolvedValueOnce({ token: "t" });
    const { result } = renderHook(() => useHostSession());
    await act(async () => { await result.current.connectSession({ userId: "u1" }); });
    act(() => socketInstances[0].fireOpen());
    act(() => result.current.skipVoiceSetup());
    await act(async () => { await result.current.startRecording(); });

    act(() => socketInstances[0].fireClose());
    expect(result.current.status).toBe("disconnected");
    expect(pipelineInstances[0].stopped).toBe(true);
  });

  it("error message → error field populated, status unchanged (sad)", async () => {
    mockedGetAuthToken.mockResolvedValueOnce({ token: "t" });
    const { result } = renderHook(() => useHostSession());
    await act(async () => { await result.current.connectSession({ userId: "u1" }); });
    act(() => socketInstances[0].fireOpen());
    act(() => socketInstances[0].fireMessage({ type: "error", message: "backend blew up" }));
    expect(result.current.error).toBe("backend blew up");
  });

  it("closeSession sends host:end + closes WS + stops pipeline", async () => {
    mockedGetAuthToken.mockResolvedValueOnce({ token: "t" });
    const { result } = renderHook(() => useHostSession());
    await act(async () => { await result.current.connectSession({ userId: "u1" }); });
    act(() => socketInstances[0].fireOpen());
    act(() => result.current.skipVoiceSetup());
    await act(async () => { await result.current.startRecording(); });

    act(() => result.current.closeSession());
    expect(socketInstances[0].sent[0]).toEqual({ type: "host:end" });
    expect(pipelineInstances[0].stopped).toBe(true);
  });

  it("unknown msg.type ignored (sad: forward compat)", async () => {
    mockedGetAuthToken.mockResolvedValueOnce({ token: "t" });
    const { result } = renderHook(() => useHostSession());
    await act(async () => { await result.current.connectSession({ userId: "u1" }); });
    act(() => socketInstances[0].fireOpen());
    const before = { ...result.current };
    act(() => socketInstances[0].fireMessage({ type: "future_event_xyz" }));
    expect(result.current.status).toBe(before.status);
    expect(result.current.error).toBe(before.error);
  });
});
