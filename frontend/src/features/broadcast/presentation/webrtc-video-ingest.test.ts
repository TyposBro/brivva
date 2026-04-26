import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { _setAppConfig } from "../../../core/config/app-config";
import { WebRtcVideoIngest } from "./webrtc-video-ingest";

const videoTrack = { kind: "video" } as MediaStreamTrack;

class FakePeerConnection {
  static instances: FakePeerConnection[] = [];

  iceGatheringState: RTCIceGatheringState = "complete";
  localDescription: RTCSessionDescriptionInit | null = null;
  readonly close = vi.fn();
  readonly addEventListener = vi.fn();
  readonly removeEventListener = vi.fn();
  readonly setRemoteDescription = vi.fn();
  readonly setCodecPreferences = vi.fn();
  readonly sender = {
    track: videoTrack,
    getParameters: vi.fn(() => ({})),
    setParameters: vi.fn(),
  };

  constructor() {
    FakePeerConnection.instances.push(this);
  }

  addTrack = vi.fn(() => this.sender);
  getTransceivers = vi.fn(() => [
    {
      sender: this.sender,
      setCodecPreferences: this.setCodecPreferences,
    },
  ]);
  createOffer = vi.fn(async () => ({ type: "offer" as const, sdp: "offer-sdp" }));
  setLocalDescription = vi.fn(async (offer: RTCSessionDescriptionInit) => {
    this.localDescription = offer;
  });
}

function streamWithTracks(tracks: MediaStreamTrack[]): MediaStream {
  return {
    getVideoTracks: () => tracks,
  } as unknown as MediaStream;
}

beforeEach(() => {
  FakePeerConnection.instances = [];
  _setAppConfig({
    workersApiBase: "http://workers.test",
    mediaWsBase: "ws://media.test",
    mediaHttpBase: "https://media.test",
    videoIngest: "webrtc",
  });
  vi.stubGlobal("RTCPeerConnection", FakePeerConnection);
  vi.stubGlobal("RTCRtpSender", {
    getCapabilities: vi.fn(() => ({
      codecs: [
        { mimeType: "video/VP8" },
        { mimeType: "video/H264" },
      ],
    })),
  });
  vi.stubGlobal(
    "fetch",
    vi.fn(async () => ({
      ok: true,
      text: async () => "answer-sdp",
    })),
  );
});

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("WebRtcVideoIngest", () => {
  it("posts an H.264 WHIP offer and applies the answer", async () => {
    const ingest = new WebRtcVideoIngest();

    await ingest.start({
      stream: streamWithTracks([videoTrack]),
      token: "jwt-token",
      sessionId: "sess-1",
      sourceLang: "ko",
    });

    const pc = FakePeerConnection.instances[0]!;
    expect(pc.addTrack).toHaveBeenCalledWith(expect.objectContaining({ kind: "video" }), expect.anything());
    expect(pc.setCodecPreferences).toHaveBeenCalledWith([{ mimeType: "video/H264" }]);
    expect(pc.sender.setParameters).toHaveBeenCalledWith({
      encodings: [{ maxBitrate: 8_000_000, maxFramerate: 30 }],
    });
    const [url, init] = vi.mocked(fetch).mock.calls[0]!;
    expect(String(url)).toBe(
      "https://media.test/whip/session?token=jwt-token&sourceLang=ko&videoMode=webrtc-h264&sessionId=sess-1",
    );
    expect(init).toEqual({
      method: "POST",
      headers: { "content-type": "application/sdp" },
      body: "offer-sdp",
    });
    expect(pc.setRemoteDescription).toHaveBeenCalledWith({ type: "answer", sdp: "answer-sdp" });
  });

  it("closes the previous peer before starting a new one", async () => {
    const ingest = new WebRtcVideoIngest();
    await ingest.start({ stream: streamWithTracks([videoTrack]), token: "one" });
    const first = FakePeerConnection.instances[0]!;

    await ingest.start({ stream: streamWithTracks([videoTrack]), token: "two" });
    ingest.stop();

    expect(first.close).toHaveBeenCalledTimes(1);
    expect(FakePeerConnection.instances[1]!.close).toHaveBeenCalledTimes(1);
  });

  it("rejects streams without a video track", async () => {
    await expect(
      new WebRtcVideoIngest().start({ stream: streamWithTracks([]), token: "jwt-token" }),
    ).rejects.toThrow("No webcam video track available");
  });

  it("rejects browsers without H.264 send support", async () => {
    vi.stubGlobal("RTCRtpSender", {
      getCapabilities: vi.fn(() => ({ codecs: [{ mimeType: "video/VP8" }] })),
    });

    await expect(
      new WebRtcVideoIngest().start({ stream: streamWithTracks([videoTrack]), token: "jwt-token" }),
    ).rejects.toThrow("Browser did not expose H.264 WebRTC encoding");
  });
});
