import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

const navigate = vi.fn();
vi.mock("react-router-dom", async (orig) => {
  const actual = await orig<typeof import("react-router-dom")>();
  return { ...actual, useNavigate: () => navigate };
});

const fetchOnboardingState = vi.fn();
const completeOnboarding = vi.fn();
vi.mock("../data/onboarding-api", () => ({
  fetchOnboardingState: (...args: unknown[]) => fetchOnboardingState(...args),
  completeOnboarding: (...args: unknown[]) => completeOnboarding(...args),
}));

const getUser = vi.fn();
const createVoice = vi.fn();
vi.mock("../data/api-client", async () => {
  const actual = await vi.importActual<typeof import("../data/api-client")>(
    "../data/api-client",
  );
  return {
    ...actual,
    getUser: (...args: unknown[]) => getUser(...args),
    createVoice: (...args: unknown[]) => createVoice(...args),
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
import OnboardingPage from "./onboarding-page";
import { MemoryRouter } from "react-router-dom";

class FakeStorage {
  private store = new Map<string, string>();
  getItem(k: string) { return this.store.get(k) ?? null; }
  setItem(k: string, v: string) { this.store.set(k, String(v)); }
  removeItem(k: string) { this.store.delete(k); }
}

function fullUser(overrides: Partial<Record<string, unknown>> = {}) {
  return {
    id: "u1",
    youtube_connected: false,
    youtube_channel_name: null,
    youtube_channel_id: null,
    email: "u1@example.com",
    name: "Aziz",
    picture: null,
    onboarding_completed_at: null,
    active_voice_id: null,
    billing_tier: "self_serve",
    bills_to: null,
    created_at: 1,
    ...overrides,
  };
}

function renderPage() {
  return render(
    <MemoryRouter initialEntries={["/onboarding"]}>
      <OnboardingPage />
    </MemoryRouter>,
  );
}

describe("OnboardingPage", () => {
  beforeEach(() => {
    _resetForTesting();
    vi.stubGlobal("localStorage", new FakeStorage());
    signIn("u1");
    navigate.mockReset();
    fetchOnboardingState.mockReset();
    completeOnboarding.mockReset();
    getUser.mockReset();
    createVoice.mockReset();
    recorder.elapsedSec = 0;
    recorder.isRecording = false;
    recorder.start.mockReset();
    recorder.stop.mockReset();
  });

  it("redirects to /dashboard when onboarding already completed (happy)", async () => {
    fetchOnboardingState.mockResolvedValue({ onboardingCompletedAt: 1700 });
    getUser.mockResolvedValue(fullUser({ onboarding_completed_at: 1700 }));
    renderPage();
    await waitFor(() =>
      expect(navigate).toHaveBeenCalledWith("/dashboard", { replace: true }),
    );
  });

  it("happy: 3-step wizard reaches /dashboard via completeOnboarding", async () => {
    fetchOnboardingState.mockResolvedValue({ onboardingCompletedAt: null });
    getUser.mockResolvedValue(fullUser());
    completeOnboarding.mockResolvedValue(undefined);
    renderPage();
    // Step 1 — platform. Skip.
    expect(await screen.findByRole("heading", { name: /Connect a streaming platform/i })).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: /Skip for now/i }));
    // Step 2 — voice. Skip.
    expect(await screen.findByRole("heading", { name: /Clone your voice/i })).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: /Skip for now/i }));
    // Step 3 — language. Default 'en' selected.
    expect(await screen.findByRole("heading", { name: /default target language/i })).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: /Finish/i }));
    await waitFor(() =>
      expect(completeOnboarding).toHaveBeenCalledWith({
        userId: "u1",
        defaultTargetLang: "en",
      }),
    );
    await waitFor(() =>
      expect(navigate).toHaveBeenCalledWith("/dashboard", { replace: true }),
    );
  });

  it("sad: completeOnboarding failure surfaces error banner", async () => {
    fetchOnboardingState.mockResolvedValue({ onboardingCompletedAt: null });
    getUser.mockResolvedValue(fullUser());
    completeOnboarding.mockRejectedValue(new Error("server down"));
    renderPage();
    await screen.findByRole("heading", { name: /Connect a streaming platform/i });
    await userEvent.click(screen.getByRole("button", { name: /Skip for now/i }));
    await screen.findByRole("heading", { name: /Clone your voice/i });
    await userEvent.click(screen.getByRole("button", { name: /Skip for now/i }));
    await screen.findByRole("heading", { name: /default target language/i });
    await userEvent.click(screen.getByRole("button", { name: /Finish/i }));
    expect(await screen.findByText(/server down/)).toBeInTheDocument();
  });
});
