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

  it("renders voice/source-lang mismatch banner when voice.source_lang !== sourceLang (sad)", async () => {
    // Dashboard's `sourceLang` state defaults to "ko" (see state init). A
    // voice enrolled in English means the clone would synthesize Korean
    // with an English accent — Workers rejects with 400, the FE catches it
    // upfront here.
    getUser.mockResolvedValue(
      fullUser({ active_voice_id: "v-en" }),
    );
    listVoices.mockResolvedValue({
      voices: [
        {
          id: "v-en",
          user_id: "u1",
          elevenlabs_voice_id: "el-en",
          name: "Aziz English",
          source_lang: "en",
          created_at: 1,
        },
      ],
    });
    listSessions.mockResolvedValue({ sessions: [] });
    listCredentials.mockResolvedValue({ credentials: [] });
    fetchOnboardingState.mockResolvedValue({ onboardingCompletedAt: 1 });

    render(
      <MemoryRouter initialEntries={["/dashboard"]}>
        <DashboardPage />
      </MemoryRouter>,
    );

    const banner = await screen.findByTestId("voice-lang-mismatch-banner");
    expect(banner).toBeInTheDocument();
    // Mentions both languages so the host knows which way to reconcile.
    expect(banner.textContent).toMatch(/English/);
    expect(banner.textContent).toMatch(/Korean/);
    // Both CTAs render.
    expect(
      screen.getByRole("button", { name: /Re-record voice/i }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: /Change session source to English/i }),
    ).toBeInTheDocument();
  });

  it("filters Grip rows out of /api/credentials — legacy rows never pre-fill (sad)", async () => {
    // Grip stream keys are one-shot per broadcast (AWS IVS rejects a
    // duplicate publisher — the second session silently dies after ~25s).
    // The save path has been removed, but legacy rows may still exist in
    // platform_credentials from before this change. The dashboard must
    // drop `grip` rows on load so nothing pre-fills the destination card.
    getUser.mockResolvedValue(fullUser());
    listVoices.mockResolvedValue({ voices: [] });
    listSessions.mockResolvedValue({ sessions: [] });
    listCredentials.mockResolvedValue({
      credentials: [
        {
          id: "pc-grip-legacy",
          user_id: "u1",
          platform: "grip",
          rtmp_url: "rtmps://stale.example/app",
          stream_key: "stale-one-shot-key",
          display_name: "grip:legacy",
          created_at: 1,
          updated_at: 1,
        },
        {
          id: "pc-tiktok",
          user_id: "u1",
          platform: "tiktok",
          rtmp_url: "rtmps://push.tiktokcdn.com/game/",
          stream_key: "tiktok-reusable",
          display_name: "tiktok:main",
          created_at: 1,
          updated_at: 1,
        },
      ],
    });
    fetchOnboardingState.mockResolvedValue({ onboardingCompletedAt: 1 });

    render(
      <MemoryRouter initialEntries={["/dashboard"]}>
        <DashboardPage />
      </MemoryRouter>,
    );
    // Wait for data load.
    await screen.findByText("BRIVVA");
    const userEvent = (await import("@testing-library/user-event")).default;
    const u = userEvent.setup();

    // Add a Grip destination. If the legacy row leaked into savedCreds it
    // would pre-fill the rtmp_url + stream_key inputs — assert they're
    // empty (= filter worked).
    await u.click(await screen.findByRole("button", { name: /Add destination/i }));
    await u.click(screen.getByRole("button", { name: /^Grip$/i }));
    const serverUrl = screen.getByPlaceholderText(/Server URL/i) as HTMLInputElement;
    const streamKey = screen.getByPlaceholderText(/Stream Key/i) as HTMLInputElement;
    expect(serverUrl.value).toBe("");
    expect(streamKey.value).toBe("");

    // And the ephemeral-key warning renders so the host knows why.
    expect(screen.getByTestId("grip-ephemeral-warning")).toBeInTheDocument();

    // Regression guard: TikTok flowed through to savedCreds (only Grip is
    // filtered). The TikTok card mounts collapsed when savedCreds[tiktok]
    // is present, so assert the Pre-filled badge (which only renders when
    // hasSavedCreds is true) shows up once expanded. Remove the Grip card
    // first so role queries on the new TikTok card aren't ambiguous.
    const removeBtns = document.querySelectorAll("button.hover\\:text-error");
    await u.click(removeBtns[0]!);
    await u.click(screen.getByRole("button", { name: /Add destination/i }));
    await u.click(screen.getByRole("button", { name: /^TikTok$/i }));
    // Expand the collapsed TikTok card. The config-toggle chevron is the
    // lucide-chevron-right button next to the X remove button.
    const chevron = document.querySelector(
      "button > svg.lucide-chevron-right",
    )?.parentElement as HTMLButtonElement | null;
    expect(chevron).not.toBeNull();
    await u.click(chevron!);
    expect(
      screen.getByText(/Pre-filled from saved credentials/i),
    ).toBeInTheDocument();
    const ttStreamKey = screen.getByPlaceholderText(
      /Stream Key/i,
    ) as HTMLInputElement;
    expect(ttStreamKey.value).toBe("tiktok-reusable");
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
