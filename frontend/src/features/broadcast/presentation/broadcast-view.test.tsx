import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor, act } from "@testing-library/react";

const navigate = vi.fn();
vi.mock("react-router-dom", async (orig) => {
  const actual = await orig<typeof import("react-router-dom")>();
  return { ...actual, useNavigate: () => navigate };
});

const session = {
  status: "ready" as const,
  liveTranscript: "",
  utterances: [{ id: 1, transcript: "hello" }],
  translations: { ja: { id: 1, text: "こんにちは" } },
  analyser: {
    fftSize: 512,
    frequencyBinCount: 256,
    getByteFrequencyData: (b: Uint8Array) => b.fill(0),
    getByteTimeDomainData: (b: Uint8Array) => b.fill(128),
  } as unknown as AnalyserNode,
  error: null,
  voiceReady: true,
  timings: [],
  videoRef: { current: null },
  connectSession: vi.fn().mockResolvedValue(undefined),
  startRecording: vi.fn().mockResolvedValue(undefined),
  stopRecording: vi.fn(),
  closeSession: vi.fn(),
  startVoiceRecording: vi.fn(),
  stopVoiceRecording: vi.fn(),
  skipVoiceSetup: vi.fn(),
  voiceElapsedSec: 0,
  voiceIsRecording: false,
  voiceMinSec: 30,
  voiceMaxSec: 180,
  setActiveTargetLangs: vi.fn(),
};

vi.mock("./use-host-session", async (orig) => {
  const actual = await orig<typeof import("./use-host-session")>();
  return {
    ...actual,
    useHostSession: () => session,
  };
});

const getSession = vi.fn();
vi.mock("../data/api-client", async () => {
  const actual = await vi.importActual<typeof import("../data/api-client")>(
    "../data/api-client",
  );
  return {
    ...actual,
    getSession: (...args: unknown[]) => getSession(...args),
  };
});

import { BroadcastView } from "./broadcast-view";
import { MemoryRouter } from "react-router-dom";

describe("BroadcastView", () => {
  beforeEach(() => {
    navigate.mockReset();
    getSession.mockReset();
    session.connectSession.mockClear();
    session.skipVoiceSetup.mockClear();
    session.closeSession.mockClear();
    HTMLCanvasElement.prototype.getContext = vi.fn(() => ({
      fillStyle: "",
      fillRect: vi.fn(),
      strokeStyle: "",
      lineWidth: 0,
      beginPath: vi.fn(),
      moveTo: vi.fn(),
      lineTo: vi.fn(),
      stroke: vi.fn(),
    })) as unknown as typeof HTMLCanvasElement.prototype.getContext;
    vi.stubGlobal("requestAnimationFrame", vi.fn(() => 1));
    vi.stubGlobal("cancelAnimationFrame", vi.fn());
  });

  it("happy: connects on mount, displays utterance + translation", async () => {
    getSession.mockResolvedValue({
      session: {
        id: "s1",
        user_id: "u1",
        voice_id: "v1",
        title: "Live test",
        source_lang: "en",
        target_langs: '["ja"]',
        status: "live",
        live_session_id: null,
        created_at: 1,
      },
      streams: [
        {
          id: "st1",
          session_id: "s1",
          lang: "ja",
          platform: "custom",
          rtmp_url: "rtmp://x/",
          stream_key: "k",
          status: "ready",
          delay_ms: 1500,
          host_gain: 0.2,
          created_at: 1,
        },
      ],
    });

    render(
      <MemoryRouter>
        <BroadcastView sessionId="s1" sourceLang="en" userId="u1" />
      </MemoryRouter>,
    );

    await waitFor(() => expect(session.connectSession).toHaveBeenCalled());
    expect(screen.getByText("hello")).toBeInTheDocument();
    expect(screen.getByText("こんにちは")).toBeInTheDocument();
  });

  it("autoSkipVoice fires skipVoiceSetup when status hits voice_setup", async () => {
    getSession.mockResolvedValue({ session: null, streams: [] });
    session.status = "voice_setup" as unknown as "ready";
    render(
      <MemoryRouter>
        <BroadcastView sessionId="s1" sourceLang="en" userId="u1" autoSkipVoice />
      </MemoryRouter>,
    );
    await waitFor(() => expect(session.skipVoiceSetup).toHaveBeenCalled());
    session.status = "ready" as const;
  });

  it("Back button closes the session and navigates (sad)", async () => {
    getSession.mockResolvedValue({ session: null, streams: [] });
    render(
      <MemoryRouter>
        <BroadcastView sessionId="s1" sourceLang="en" userId="u1" />
      </MemoryRouter>,
    );
    const back = await screen.findByRole("button", { name: /Back/i });
    await act(async () => {
      back.click();
    });
    expect(session.closeSession).toHaveBeenCalled();
    expect(navigate).toHaveBeenCalledWith("/session/s1");
  });
});
