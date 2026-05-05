import { describe, it, expect, beforeEach, vi } from "vitest";
import { act, renderHook, waitFor } from "@testing-library/react";

// Mock api + auth store. ensureFreshToken is the seam that connectSession
// goes through to obtain a JWT — keeping it as a vi.fn() lets each test
// scenario simulate happy / sad cases without touching real fetch.
vi.mock("../data/api-client", () => ({
  cloneSessionVoice: vi.fn(),
}));
vi.mock("../../../shared/auth/auth-store", () => ({
  ensureFreshToken: vi.fn(),
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

const { pipelineInstances, socketInstances, FakePipeline, FakeSocket } =
  vi.hoisted(() => {
    const pipelineInstances: PipeInst[] = [];
    const socketInstances: SockInst[] = [];

    class FakePipeline {
      started = false;
      stopped = false;
      onAudio: ((b: ArrayBuffer) => void) | null = null;
      constructor() {
        pipelineInstances.push(this);
      }
      async start(onAudio: (b: ArrayBuffer) => void) {
        this.started = true;
        this.onAudio = onAudio;
        return { fftSize: 512 } as unknown as AnalyserNode;
      }
      stop() {
        this.stopped = true;
      }
    }

    class FakeSocket {
      callbacks: Callbacks | null = null;
      params: Record<string, string> | null = null;
      sent: unknown[] = [];
      audio: ArrayBuffer[] = [];
      _open = false;
      constructor() {
        socketInstances.push(this);
      }
      connect(params: Record<string, string>, cbs: Callbacks) {
        this.params = params;
        this.callbacks = cbs;
      }
      get isOpen() {
        return this._open;
      }
      fireOpen() {
        this._open = true;
        this.callbacks?.onOpen?.();
      }
      fireMessage(m: unknown) {
        this.callbacks?.onMessage(m);
      }
      fireClose() {
        this._open = false;
        this.callbacks?.onClose();
      }
      sendJson(m: unknown) {
        this.sent.push(m);
      }
      sendAudio(b: ArrayBuffer) {
        this.audio.push(b);
      }
      close() {
        this._open = false;
      }
    }

    return { pipelineInstances, socketInstances, FakePipeline, FakeSocket };
  });

vi.mock("../../../shared/audio/audio-pipeline", () => ({
  AudioPipeline: FakePipeline,
}));
vi.mock("../../../shared/networking/session-socket", () => ({
  SessionSocket: FakeSocket,
}));

import * as api from "../data/api-client";
import { ensureFreshToken } from "../../../shared/auth/auth-store";
import { useHostSession } from "./use-host-session";

const mockedEnsureFreshToken = ensureFreshToken as unknown as ReturnType<
  typeof vi.fn
>;
const mockedCloneSessionVoice = api.cloneSessionVoice as unknown as ReturnType<
  typeof vi.fn
>;

function installMedia() {
  const track = {
    kind: "video",
    stop: vi.fn(),
    getSettings: () => ({ width: 720, height: 1280, frameRate: 30 }),
  };
  const stream = {
    getTracks: () => [track],
    getVideoTracks: () => [track],
  } as unknown as MediaStream;
  Object.defineProperty(globalThis.navigator, "mediaDevices", {
    configurable: true,
    value: { getUserMedia: vi.fn(async () => stream) },
  });

  class FakeScriptProcessorNode {
    onaudioprocess:
      | ((event: {
          inputBuffer: { getChannelData: () => Float32Array };
        }) => void)
      | null = null;
    connect() {}
    disconnect() {}
  }

  class FakeMediaStreamSourceNode {
    connect() {}
    disconnect() {}
  }

  class FakeAudioContext {
    destination = {};
    createMediaStreamSource() {
      return new FakeMediaStreamSourceNode();
    }
    createScriptProcessor() {
      return new FakeScriptProcessorNode();
    }
    close() {
      return Promise.resolve();
    }
  }

  Object.defineProperty(globalThis, "AudioContext", {
    configurable: true,
    value: FakeAudioContext,
  });

  class FakeRTCPeerConnection extends EventTarget {
    localDescription: RTCSessionDescriptionInit | null = null;
    iceGatheringState: RTCIceGatheringState = "complete";
    transceiver = {
      sender: {
        track,
        getParameters: () => ({ encodings: [] }),
        setParameters: vi.fn(async () => undefined),
      },
      setCodecPreferences: vi.fn(),
    };
    addTrack() {
      return this.transceiver.sender;
    }
    getTransceivers() {
      return [this.transceiver];
    }
    getSenders() {
      return [];
    }
    async createOffer() {
      return { type: "offer" as const, sdp: "offer-sdp" };
    }
    async setLocalDescription(desc: RTCSessionDescriptionInit) {
      this.localDescription = desc;
    }
    async setRemoteDescription() {}
    close() {}
  }
  Object.defineProperty(globalThis, "RTCPeerConnection", {
    configurable: true,
    value: FakeRTCPeerConnection,
  });
  Object.defineProperty(globalThis, "RTCRtpSender", {
    configurable: true,
    value: {
      getCapabilities: vi.fn(() => ({
        codecs: [
          {
            mimeType: "video/H264",
            sdpFmtpLine: "packetization-mode=1;profile-level-id=42e01f",
          },
        ],
      })),
    },
  });
}

describe("useHostSession", () => {
  beforeEach(() => {
    pipelineInstances.length = 0;
    socketInstances.length = 0;
    mockedEnsureFreshToken.mockReset();
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
    mockedEnsureFreshToken.mockResolvedValueOnce("tok_1");
    const { result } = renderHook(() => useHostSession());

    await act(async () => {
      await result.current.connectSession({
        userId: "u1",
        sessionId: "s1",
        sourceLang: "en",
      });
    });

    expect(mockedEnsureFreshToken).toHaveBeenCalledTimes(1);
    const connected = socketInstances.filter((s) => s.callbacks);
    expect(connected).toHaveLength(1);
    expect(connected[0].params).toMatchObject({
      sourceLang: "en",
      token: "tok_1",
      sessionId: "s1",
    });
    expect(result.current.status).toBe("creating");

    act(() => socketInstances[0].fireOpen());
    expect(result.current.status).toBe("voice_setup");
  });

  it("auth token fetch failure (sad)", async () => {
    mockedEnsureFreshToken.mockRejectedValueOnce(new Error("401 unauthorized"));
    const { result } = renderHook(() => useHostSession());

    await act(async () => {
      await result.current.connectSession({ userId: "u1" });
    });
    expect(result.current.error).toContain("Auth token fetch failed");
    expect(result.current.error).toContain("401");
    expect(socketInstances.filter((s) => s.callbacks)).toHaveLength(0);
  });

  it("skipVoiceSetup → ready", async () => {
    mockedEnsureFreshToken.mockResolvedValueOnce("t");
    const { result } = renderHook(() => useHostSession());
    await act(async () => {
      await result.current.connectSession({ userId: "u1" });
    });
    act(() => socketInstances[0].fireOpen());
    act(() => result.current.skipVoiceSetup());
    expect(result.current.status).toBe("ready");
  });

  it("stopVoiceRecording clones through Workers for the active session", async () => {
    mockedEnsureFreshToken.mockResolvedValueOnce("t");
    mockedCloneSessionVoice.mockResolvedValueOnce({
      voice: {
        id: "v1",
        user_id: "u1",
        elevenlabs_voice_id: "el1",
        name: "n",
        created_at: 1,
      },
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
    await act(async () => {
      await result.current.startRecording();
    });
    expect(result.current.status).toBe("idle");
    expect(pipelineInstances.every((p) => !p.started)).toBe(true);
  });

  it("startRecording when open → pipeline starts, status=recording, audio forwarded", async () => {
    mockedEnsureFreshToken.mockResolvedValueOnce("t");
    const { result } = renderHook(() => useHostSession());
    await act(async () => {
      await result.current.connectSession({ userId: "u1" });
    });
    act(() => socketInstances[0].fireOpen());
    act(() => result.current.skipVoiceSetup());

    await act(async () => {
      await result.current.startRecording();
    });
    expect(result.current.status).toBe("recording");
    expect(result.current.analyser).not.toBeNull();

    const pipe = pipelineInstances[0];
    const buf = new ArrayBuffer(8);
    pipe.onAudio!(buf);
    expect(socketInstances[0].audio).toEqual([buf]);
    expect(socketInstances[0].sent).toContainEqual({
      type: "webrtc:offer",
      sdp: "offer-sdp",
      videoProfile: { width: 720, height: 1280, fps: 30 },
    });
  });

  it("startRecording without H.264 → stays ready and shows supported-device guidance", async () => {
    Object.defineProperty(globalThis, "RTCRtpSender", {
      configurable: true,
      value: {
        getCapabilities: vi.fn(() => ({
          codecs: [{ mimeType: "video/VP8", sdpFmtpLine: "" }],
        })),
      },
    });
    mockedEnsureFreshToken.mockResolvedValueOnce("t");
    const { result } = renderHook(() => useHostSession());
    await act(async () => {
      await result.current.connectSession({ userId: "u1" });
    });
    act(() => socketInstances[0].fireOpen());
    act(() => result.current.skipVoiceSetup());

    await act(async () => {
      await result.current.startRecording();
    });

    expect(result.current.status).toBe("ready");
    expect(result.current.connectionIssue).toBe(
      "Your browser/device can’t provide launch-safe H.264 video for live streaming. Please use desktop Chrome or Brave.",
    );
    expect(pipelineInstances[0].stopped).toBe(true);
    expect(socketInstances[0].sent).not.toContainEqual(
      expect.objectContaining({ type: "webrtc:offer" }),
    );
  });

  it("stopRecording → pipeline stop + status=ready", async () => {
    mockedEnsureFreshToken.mockResolvedValueOnce("t");
    const { result } = renderHook(() => useHostSession());
    await act(async () => {
      await result.current.connectSession({ userId: "u1" });
    });
    act(() => socketInstances[0].fireOpen());
    act(() => result.current.skipVoiceSetup());
    await act(async () => {
      await result.current.startRecording();
    });
    act(() => result.current.stopRecording());
    expect(result.current.status).toBe("ready");
    expect(pipelineInstances[0].stopped).toBe(true);
    expect(result.current.analyser).toBeNull();
  });

  it("full pipeline: interim → final → translation → tts_end populates timings + transcripts", async () => {
    mockedEnsureFreshToken.mockResolvedValueOnce("t");
    const { result } = renderHook(() => useHostSession());
    act(() => result.current.setActiveTargetLangs(["ja"]));
    await act(async () => {
      await result.current.connectSession({ userId: "u1" });
    });
    act(() => socketInstances[0].fireOpen());
    act(() => result.current.skipVoiceSetup());
    await act(async () => {
      await result.current.startRecording();
    });

    const s = socketInstances[0];
    act(() => s.fireMessage({ type: "interim", transcript: "hel" }));
    expect(result.current.liveTranscript).toBe("hel");

    act(() =>
      s.fireMessage({
        type: "final",
        utteranceId: 1,
        transcript: "hello",
        sttMs: 200,
      }),
    );
    expect(result.current.liveTranscript).toBe("");
    expect(result.current.utterances).toEqual([{ id: 1, transcript: "hello" }]);

    act(() =>
      s.fireMessage({
        type: "translation",
        utteranceId: 1,
        targetLang: "ja",
        text: "こ",
        translateMs: 120,
      }),
    );
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
    mockedEnsureFreshToken.mockResolvedValueOnce("t");
    const { result } = renderHook(() => useHostSession());
    await act(async () => {
      await result.current.connectSession({ userId: "u1" });
    });
    act(() => socketInstances[0].fireOpen());
    act(() => result.current.skipVoiceSetup());
    await act(async () => {
      await result.current.startRecording();
    });

    act(() => socketInstances[0].fireClose());
    expect(result.current.status).toBe("disconnected");
    expect(pipelineInstances[0].stopped).toBe(true);
  });

  it("error message → error field populated, status unchanged (sad)", async () => {
    mockedEnsureFreshToken.mockResolvedValueOnce("t");
    const { result } = renderHook(() => useHostSession());
    await act(async () => {
      await result.current.connectSession({ userId: "u1" });
    });
    act(() => socketInstances[0].fireOpen());
    act(() =>
      socketInstances[0].fireMessage({
        type: "error",
        message: "backend blew up",
      }),
    );
    expect(result.current.error).toBe("backend blew up");
  });

  it("closeSession sends host:end + closes WS + stops pipeline", async () => {
    mockedEnsureFreshToken.mockResolvedValueOnce("t");
    const { result } = renderHook(() => useHostSession());
    await act(async () => {
      await result.current.connectSession({ userId: "u1" });
    });
    act(() => socketInstances[0].fireOpen());
    act(() => result.current.skipVoiceSetup());
    await act(async () => {
      await result.current.startRecording();
    });

    act(() => result.current.closeSession());
    expect(socketInstances[0].sent).toContainEqual({ type: "host:end" });
    expect(pipelineInstances[0].stopped).toBe(true);
  });

  it("unknown msg.type ignored (sad: forward compat)", async () => {
    mockedEnsureFreshToken.mockResolvedValueOnce("t");
    const { result } = renderHook(() => useHostSession());
    await act(async () => {
      await result.current.connectSession({ userId: "u1" });
    });
    act(() => socketInstances[0].fireOpen());
    const before = { ...result.current };
    act(() => socketInstances[0].fireMessage({ type: "future_event_xyz" }));
    expect(result.current.status).toBe(before.status);
    expect(result.current.error).toBe(before.error);
  });
});
