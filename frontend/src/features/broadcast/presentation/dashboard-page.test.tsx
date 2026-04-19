import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";

const navigate = vi.fn();
vi.mock("react-router-dom", async (orig) => {
  const actual = await orig<typeof import("react-router-dom")>();
  return {
    ...actual,
    useNavigate: () => navigate,
    useSearchParams: () => [new URLSearchParams(), vi.fn()],
  };
});

const getUser = vi.fn();
const listVoices = vi.fn();
const listSessions = vi.fn();
const listCredentials = vi.fn();
vi.mock("../data/api-client", async () => {
  const actual = await vi.importActual<typeof import("../data/api-client")>(
    "../data/api-client",
  );
  return {
    ...actual,
    getUser: (...args: unknown[]) => getUser(...args),
    listVoices: (...args: unknown[]) => listVoices(...args),
    listSessions: (...args: unknown[]) => listSessions(...args),
    listCredentials: (...args: unknown[]) => listCredentials(...args),
  };
});

const fetchOnboardingState = vi.fn();
vi.mock("../data/onboarding-api", () => ({
  fetchOnboardingState: (...args: unknown[]) => fetchOnboardingState(...args),
}));

vi.mock("../../../shared/audio/voice-recorder", () => ({
  useVoiceRecorder: vi.fn(() => ({
    start: vi.fn(),
    stop: vi.fn(),
    elapsedSec: 0,
    isRecording: false,
  })),
}));

import { signIn, _resetForTesting } from "../../../shared/auth/auth-store";
import DashboardPage from "./dashboard-page";
import { MemoryRouter } from "react-router-dom";

class FakeStorage {
  private store = new Map<string, string>();
  getItem(k: string) { return this.store.get(k) ?? null; }
  setItem(k: string, v: string) { this.store.set(k, String(v)); }
  removeItem(k: string) { this.store.delete(k); }
}

function fullUser(over: Partial<Record<string, unknown>> = {}) {
  return {
    id: "u1",
    youtube_connected: false,
    youtube_channel_name: null,
    youtube_channel_id: null,
    email: null,
    name: null,
    picture: null,
    onboarding_completed_at: 1,
    active_voice_id: null,
    billing_tier: "self_serve",
    bills_to: null,
    created_at: 1,
    ...over,
  };
}

describe("DashboardPage", () => {
  beforeEach(() => {
    _resetForTesting();
    vi.stubGlobal("localStorage", new FakeStorage());
    signIn("u1");
    navigate.mockReset();
    getUser.mockReset();
    listVoices.mockReset();
    listSessions.mockReset();
    listCredentials.mockReset();
    fetchOnboardingState.mockReset();
  });

  it("happy: renders header + Go Live disabled with no destinations", async () => {
    getUser.mockResolvedValue(fullUser());
    listVoices.mockResolvedValue({ voices: [] });
    listSessions.mockResolvedValue({ sessions: [] });
    listCredentials.mockResolvedValue({ credentials: [] });
    fetchOnboardingState.mockResolvedValue({ onboardingCompletedAt: 1 });

    render(
      <MemoryRouter initialEntries={["/dashboard"]}>
        <DashboardPage />
      </MemoryRouter>,
    );
    expect(await screen.findByText("BRIVVA")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: /Add a destination to go live/i }),
    ).toBeDisabled();
  });

  it("redirects to /onboarding when not yet completed (sad)", async () => {
    getUser.mockResolvedValue(fullUser({ onboarding_completed_at: null }));
    listVoices.mockResolvedValue({ voices: [] });
    listSessions.mockResolvedValue({ sessions: [] });
    listCredentials.mockResolvedValue({ credentials: [] });
    fetchOnboardingState.mockResolvedValue({ onboardingCompletedAt: null });

    render(
      <MemoryRouter initialEntries={["/dashboard"]}>
        <DashboardPage />
      </MemoryRouter>,
    );
    await waitFor(() =>
      expect(navigate).toHaveBeenCalledWith("/onboarding", { replace: true }),
    );
  });

  it("settings drawer toggles the YouTube + Voice sections (happy)", async () => {
    getUser.mockResolvedValue(fullUser({ youtube_connected: true, youtube_channel_name: "Aziz" }));
    listVoices.mockResolvedValue({ voices: [] });
    listSessions.mockResolvedValue({ sessions: [] });
    listCredentials.mockResolvedValue({ credentials: [] });
    fetchOnboardingState.mockResolvedValue({ onboardingCompletedAt: 1 });

    render(
      <MemoryRouter initialEntries={["/dashboard"]}>
        <DashboardPage />
      </MemoryRouter>,
    );
    await screen.findByText("BRIVVA");
    // Settings toggle button has only an icon — pick by querying the cog SVG's button.
    const buttons = screen.getAllByRole("button");
    const settingsBtn = buttons.find((b) => b.querySelector(".lucide-settings2"));
    expect(settingsBtn).toBeDefined();
    const userEvent = (await import("@testing-library/user-event")).default;
    await userEvent.setup().click(settingsBtn!);
    expect(await screen.findByText(/Settings/)).toBeInTheDocument();
    expect(screen.getByText("Aziz")).toBeInTheDocument();
  });

  it("Add destination opens a flat platform picker (no regional grouping)", async () => {
    getUser.mockResolvedValue(fullUser());
    listVoices.mockResolvedValue({ voices: [] });
    listSessions.mockResolvedValue({ sessions: [] });
    listCredentials.mockResolvedValue({ credentials: [] });
    fetchOnboardingState.mockResolvedValue({ onboardingCompletedAt: 1 });

    render(
      <MemoryRouter initialEntries={["/dashboard"]}>
        <DashboardPage />
      </MemoryRouter>,
    );
    const userEvent = (await import("@testing-library/user-event")).default;
    const u = userEvent.setup();
    const addBtn = await screen.findByRole("button", { name: /Add destination/i });
    await u.click(addBtn);
    expect(screen.getByPlaceholderText(/Paste RTMP URL/i)).toBeInTheDocument();
    // Regional groupings are gone — no "Korean Platforms" heading.
    expect(screen.queryByText(/Korean Platforms/i)).not.toBeInTheDocument();
    expect(screen.queryByText(/Japanese Platforms/i)).not.toBeInTheDocument();
    expect(screen.queryByText(/Chinese Platforms/i)).not.toBeInTheDocument();
    // Flat picker exposes platforms directly.
    expect(screen.getByRole("button", { name: /YouTube/i })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /TikTok/i })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Bilibili/i })).toBeInTheDocument();
  });

  it("adds a destination with target_lang seeded from default-target-lang storage", async () => {
    getUser.mockResolvedValue(fullUser());
    listVoices.mockResolvedValue({ voices: [] });
    listSessions.mockResolvedValue({ sessions: [] });
    listCredentials.mockResolvedValue({ credentials: [] });
    fetchOnboardingState.mockResolvedValue({ onboardingCompletedAt: 1 });

    const storage = new FakeStorage();
    storage.setItem("brivva_default_target_lang", "ja");
    vi.stubGlobal("localStorage", storage);
    signIn("u1");

    render(
      <MemoryRouter initialEntries={["/dashboard"]}>
        <DashboardPage />
      </MemoryRouter>,
    );
    const userEvent = (await import("@testing-library/user-event")).default;
    const u = userEvent.setup();
    const addBtn = await screen.findByRole("button", { name: /Add destination/i });
    await u.click(addBtn);
    await u.click(screen.getByRole("button", { name: /^TikTok$/i }));

    // Per-destination dropdown reflects the onboarding default.
    // Combobox order: [0] source-lang select, [1] new destination's lang select.
    const selects = await screen.findAllByRole("combobox");
    expect((selects[1] as HTMLSelectElement).value).toBe("ja");
  });

  it("recent sessions render in the list (happy)", async () => {
    getUser.mockResolvedValue(fullUser());
    listVoices.mockResolvedValue({ voices: [] });
    listSessions.mockResolvedValue({
      sessions: [
        {
          id: "s1",
          user_id: "u1",
          voice_id: null,
          title: "Past one",
          source_lang: "ko",
          target_langs: '["zh"]',
          status: "ended",
          live_session_id: null,
          created_at: 1,
        },
      ],
    });
    listCredentials.mockResolvedValue({ credentials: [] });
    fetchOnboardingState.mockResolvedValue({ onboardingCompletedAt: 1 });

    render(
      <MemoryRouter initialEntries={["/dashboard"]}>
        <DashboardPage />
      </MemoryRouter>,
    );
    expect(await screen.findByText("Past one")).toBeInTheDocument();
  });
});
