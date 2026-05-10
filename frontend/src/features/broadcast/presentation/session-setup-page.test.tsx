import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

const navigate = vi.fn();
vi.mock("react-router-dom", async (orig) => {
  const actual = await orig<typeof import("react-router-dom")>();
  return {
    ...actual,
    useNavigate: () => navigate,
    useParams: () => ({ id: "s1" }),
  };
});

const getSession = vi.fn();
const cloneSessionVoice = vi.fn();
const updateSessionVoicePreset = vi.fn();
vi.mock("../data/api-client", async () => {
  const actual = await vi.importActual<typeof import("../data/api-client")>(
    "../data/api-client",
  );
  return {
    ...actual,
    getSession: (...args: unknown[]) => getSession(...args),
    cloneSessionVoice: (...args: unknown[]) => cloneSessionVoice(...args),
    updateSessionVoicePreset: (...args: unknown[]) => updateSessionVoicePreset(...args),
  };
});

const recorder = {
  start: vi.fn(),
  stop: vi.fn(),
  elapsedSec: 0,
  isRecording: false,
};
vi.mock("../../../shared/audio/voice-recorder", () => ({
  useVoiceRecorder: vi.fn(() => recorder),
}));

import { signIn, _resetForTesting } from "../../../shared/auth/auth-store";
import SessionSetupPage from "./session-setup-page";
import { MemoryRouter } from "react-router-dom";

class FakeStorage {
  private store = new Map<string, string>();
  getItem(k: string) { return this.store.get(k) ?? null; }
  setItem(k: string, v: string) { this.store.set(k, String(v)); }
  removeItem(k: string) { this.store.delete(k); }
}

function makeSession(over: Partial<Record<string, unknown>> = {}) {
  return {
    id: "s1",
    user_id: "u1",
    voice_id: null,
    // Session was created with clone intent, so a missing voice blocks Go Live
    // until the host either finishes the clone or explicitly skips to a
    // default voice. Sessions created with `voice_preset: "female" | "male"`
    // don't need the clone step and go straight to ready.
    voice_preset: "cloned",
    title: "Setup test",
    source_lang: "ko",
    target_langs: '["zh"]',
    status: "ready",
    live_session_id: null,
    created_at: 1,
    ...over,
  };
}

describe("SessionSetupPage", () => {
  beforeEach(() => {
    _resetForTesting();
    vi.stubGlobal("localStorage", new FakeStorage());
    signIn("u1");
    navigate.mockReset();
    getSession.mockReset();
    cloneSessionVoice.mockReset();
    updateSessionVoicePreset.mockReset();
    recorder.start.mockReset();
    recorder.stop.mockReset();
    recorder.elapsedSec = 0;
    recorder.isRecording = false;
  });

  it("voice already cloned: Go Live enabled and navigates to /live (happy)", async () => {
    getSession.mockResolvedValue({
      session: makeSession({ voice_id: "v1" }),
      streams: [],
    });
    render(
      <MemoryRouter initialEntries={["/session/s1/setup"]}>
        <SessionSetupPage />
      </MemoryRouter>,
    );
    const goLive = await screen.findByRole("button", { name: /Go Live/i });
    expect(goLive).not.toBeDisabled();
    await userEvent.click(goLive);
    expect(navigate).toHaveBeenCalledWith("/session/s1/live");
  });

  it("no voice yet: Skip → Go Live unlocks (happy)", async () => {
    getSession.mockResolvedValue({ session: makeSession(), streams: [] });
    render(
      <MemoryRouter initialEntries={["/session/s1/setup"]}>
        <SessionSetupPage />
      </MemoryRouter>,
    );
    await screen.findByRole("heading", { name: /Setup test/i });
    expect(screen.getByRole("button", { name: /Go Live/i })).toBeDisabled();
    await userEvent.click(screen.getByRole("button", { name: /Skip/i }));
    expect(screen.getByRole("button", { name: /Go Live/i })).not.toBeDisabled();
  });

  it("built-in instant clone preset does not require a recorded voice", async () => {
    getSession.mockResolvedValue({
      session: makeSession({ voice_id: null, voice_preset: "yuna" }),
      streams: [],
    });
    render(
      <MemoryRouter initialEntries={["/session/s1/setup"]}>
        <SessionSetupPage />
      </MemoryRouter>,
    );

    expect(await screen.findByRole("button", { name: /Yuna/i })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Go Live/i })).not.toBeDisabled();
  });

  it("session not found renders fallback (sad)", async () => {
    getSession.mockResolvedValue({ session: null, streams: [] });
    render(
      <MemoryRouter initialEntries={["/session/missing/setup"]}>
        <SessionSetupPage />
      </MemoryRouter>,
    );
    expect(await screen.findByText(/Session not found/i)).toBeInTheDocument();
  });

  it("voice upload happy: clone + reload + voice card switches to ready", async () => {
    recorder.stop.mockReturnValue("BASE64");
    recorder.elapsedSec = 45;
    recorder.isRecording = true;
    cloneSessionVoice.mockResolvedValue({
      voice: {
        id: "v1",
        user_id: "u1",
        elevenlabs_voice_id: "el1",
        name: "n",
        source_lang: "ko",
        created_at: 1,
      },
    });
    // First load returns no voice; reload after clone returns voice attached.
    getSession
      .mockResolvedValueOnce({ session: makeSession(), streams: [] })
      .mockResolvedValue({ session: makeSession({ voice_id: "v1" }), streams: [] });
    render(
      <MemoryRouter initialEntries={["/session/s1/setup"]}>
        <SessionSetupPage />
      </MemoryRouter>,
    );
    const stopBtn = await screen.findByRole("button", { name: /Stop & Clone/i });
    await userEvent.click(stopBtn);
    await waitFor(() => expect(cloneSessionVoice).toHaveBeenCalled());
    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: /Re-record voice sample/i }),
      ).toBeInTheDocument(),
    );
  });
});
