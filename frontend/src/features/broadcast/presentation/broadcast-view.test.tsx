import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor, act } from "@testing-library/react";
import type { ProviderHealthNotice } from "./reducer";

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
  mediaDiagnostics: null,
  connectionIssue: null,
  providerHealth: [] as ProviderHealthNotice[],
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
const getProviderHealth = vi.fn();
vi.mock("../data/api-client", async () => {
  const actual = await vi.importActual<typeof import("../data/api-client")>(
    "../data/api-client",
  );
  return {
    ...actual,
    getSession: (...args: unknown[]) => getSession(...args),
    getProviderHealth: (...args: unknown[]) => getProviderHealth(...args),
  };
});

import { BroadcastView } from "./broadcast-view";
import { MemoryRouter } from "react-router-dom";

describe("BroadcastView", () => {
  beforeEach(() => {
    navigate.mockReset();
    getSession.mockReset();
    getProviderHealth.mockReset();
    session.connectSession.mockClear();
    session.skipVoiceSetup.mockClear();
    session.closeSession.mockClear();
    session.status = "ready";
    session.providerHealth = [];
    session.connectionIssue = null;
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

  it("renders provider-health notices with billable status", async () => {
    getSession.mockResolvedValue({ session: null, streams: [] });
    session.providerHealth = [
      {
        provider: "elevenlabs",
        state: "degraded",
        recoverable: true,
        billable: false,
        reason: "rate_limited",
        targetLang: "ja",
        statusCode: 429,
        message: "Japanese TTS not billable while rate limited.",
      },
    ];

    render(
      <MemoryRouter>
        <BroadcastView sessionId="s1" sourceLang="en" userId="u1" />
      </MemoryRouter>,
    );

    expect(await screen.findByText(/Provider health/i)).toBeInTheDocument();
    expect(screen.getByText(/elevenlabs/i)).toBeInTheDocument();
    expect(screen.getByText(/unbillable/i)).toBeInTheDocument();
    expect(
      screen.getByText(/Japanese TTS not billable while rate limited/i),
    ).toBeInTheDocument();
  });

  it("shows YouTube STARTING until provider confirms active ingest", async () => {
    session.status = "recording" as unknown as "ready";
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
          platform: "youtube",
          rtmp_url: "rtmps://a.rtmps.youtube.com/live2",
          stream_key: "k",
          status: "ready",
          delay_ms: 1500,
          host_gain: 0.2,
          created_at: 1,
        },
      ],
    });
    getProviderHealth.mockResolvedValue({
      sessionId: "s1",
      streams: [
        {
          streamId: "st1",
          platform: "youtube",
          provider: "youtube",
          streamStatus: "ready",
          healthStatus: "noData",
          providerConfirmedLive: false,
        },
      ],
    });

    render(
      <MemoryRouter>
        <BroadcastView sessionId="s1" sourceLang="en" userId="u1" />
      </MemoryRouter>,
    );

    expect(await screen.findByText("STARTING")).toBeInTheDocument();
    expect(screen.getByText(/YouTube not live yet: stream=ready health=noData/i)).toBeInTheDocument();
  });

  it("shows YouTube LIVE only after provider confirms active ingest", async () => {
    session.status = "recording" as unknown as "ready";
    getSession.mockResolvedValue({
      session: null,
      streams: [
        {
          id: "st1",
          session_id: "s1",
          lang: "ja",
          platform: "youtube",
          rtmp_url: "rtmps://a.rtmps.youtube.com/live2",
          stream_key: "k",
          status: "ready",
          delay_ms: 1500,
          host_gain: 0.2,
          created_at: 1,
        },
      ],
    });
    getProviderHealth.mockResolvedValue({
      sessionId: "s1",
      streams: [
        {
          streamId: "st1",
          platform: "youtube",
          provider: "youtube",
          streamStatus: "active",
          healthStatus: "noData",
          providerConfirmedLive: true,
        },
      ],
    });

    render(
      <MemoryRouter>
        <BroadcastView sessionId="s1" sourceLang="en" userId="u1" />
      </MemoryRouter>,
    );

    expect(await screen.findByText("LIVE")).toBeInTheDocument();
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
