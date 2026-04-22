import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor, fireEvent } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";

const navigate = vi.fn();
vi.mock("react-router-dom", async (orig) => {
  const actual = await orig<typeof import("react-router-dom")>();
  return {
    ...actual,
    useNavigate: () => navigate,
  };
});

const getUser = vi.fn();
const listVoices = vi.fn();
const listSessions = vi.fn();
const deleteVoice = vi.fn();
const deleteAccount = vi.fn();
vi.mock("../data/api-client", async () => {
  const actual = await vi.importActual<typeof import("../data/api-client")>(
    "../data/api-client",
  );
  return {
    ...actual,
    getUser: (...args: unknown[]) => getUser(...args),
    listVoices: (...args: unknown[]) => listVoices(...args),
    listSessions: (...args: unknown[]) => listSessions(...args),
    deleteVoice: (...args: unknown[]) => deleteVoice(...args),
    deleteAccount: (...args: unknown[]) => deleteAccount(...args),
  };
});

import { signIn, _resetForTesting } from "../../../shared/auth/auth-store";
import { ApiError } from "../data/api-client";
import SettingsPage from "./settings-page";

class FakeStorage {
  private store = new Map<string, string>();
  getItem(k: string) {
    return this.store.get(k) ?? null;
  }
  setItem(k: string, v: string) {
    this.store.set(k, String(v));
  }
  removeItem(k: string) {
    this.store.delete(k);
  }
}

function user(over: Partial<Record<string, unknown>> = {}) {
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

function voice(id: string, over: Partial<Record<string, unknown>> = {}) {
  return {
    id,
    user_id: "u1",
    elevenlabs_voice_id: `el-${id}`,
    name: `Voice ${id}`,
    source_lang: "en",
    created_at: 1,
    ...over,
  };
}

function session(id: string, over: Partial<Record<string, unknown>> = {}) {
  return {
    id,
    user_id: "u1",
    voice_id: null,
    title: "s",
    source_lang: "en",
    target_langs: '["ja"]',
    status: "ended",
    live_session_id: null,
    voice_preset: "cloned",
    created_at: 1,
    ...over,
  };
}

function renderPage() {
  return render(
    <MemoryRouter initialEntries={["/settings"]}>
      <SettingsPage />
    </MemoryRouter>,
  );
}

describe("SettingsPage", () => {
  beforeEach(() => {
    _resetForTesting();
    vi.stubGlobal("localStorage", new FakeStorage());
    signIn("u1");
    navigate.mockReset();
    getUser.mockReset();
    listVoices.mockReset();
    listSessions.mockReset();
    deleteVoice.mockReset();
    deleteAccount.mockReset();
  });

  it("renders voice clones list (happy)", async () => {
    getUser.mockResolvedValue(user({ active_voice_id: "v1" }));
    listVoices.mockResolvedValue({ voices: [voice("v1"), voice("v2")] });
    listSessions.mockResolvedValue({ sessions: [] });

    renderPage();
    await waitFor(() => expect(screen.getByText("Voice v1")).toBeInTheDocument());
    expect(screen.getByText("Voice v2")).toBeInTheDocument();
    expect(screen.getByText("Active")).toBeInTheDocument();
  });

  it("delete voice calls API and reloads (happy)", async () => {
    getUser.mockResolvedValue(user());
    listVoices.mockResolvedValueOnce({ voices: [voice("v1")] });
    listSessions.mockResolvedValue({ sessions: [] });
    deleteVoice.mockResolvedValue({ status: "deleted" });
    listVoices.mockResolvedValueOnce({ voices: [] });

    renderPage();
    await waitFor(() => expect(screen.getByText("Voice v1")).toBeInTheDocument());
    fireEvent.click(screen.getByTestId("voice-delete-v1"));
    await waitFor(() => expect(deleteVoice).toHaveBeenCalledWith("v1"));
    await waitFor(() =>
      expect(screen.queryByText("Voice v1")).not.toBeInTheDocument(),
    );
  });

  it("disables delete button when voice is referenced by an active session (sad)", async () => {
    getUser.mockResolvedValue(user());
    listVoices.mockResolvedValue({ voices: [voice("v1")] });
    listSessions.mockResolvedValue({
      sessions: [session("s1", { voice_id: "v1", status: "live" })],
    });

    renderPage();
    await waitFor(() =>
      expect(screen.getByTestId("voice-delete-v1")).toBeDisabled(),
    );
    expect(screen.getByText("In session")).toBeInTheDocument();
  });

  it("surfaces 409 from deleteVoice as a row-scoped banner (sad)", async () => {
    getUser.mockResolvedValue(user());
    // No active session in local state → button is enabled, but server says 409
    // (race between user's session list refresh and another tab going live).
    listVoices.mockResolvedValue({ voices: [voice("v1")] });
    listSessions.mockResolvedValue({ sessions: [] });
    deleteVoice.mockRejectedValue(new ApiError(409, "voice_in_use", "API 409: voice_in_use"));

    renderPage();
    await waitFor(() => expect(screen.getByText("Voice v1")).toBeInTheDocument());
    fireEvent.click(screen.getByTestId("voice-delete-v1"));
    await waitFor(() =>
      expect(
        screen.getByText(/attached to a live session/i),
      ).toBeInTheDocument(),
    );
    // Row is still there; nothing was deleted.
    expect(screen.getByText("Voice v1")).toBeInTheDocument();
  });

  it("delete account requires typed 'delete' then calls API and redirects home (happy)", async () => {
    getUser.mockResolvedValue(user());
    listVoices.mockResolvedValue({ voices: [] });
    listSessions.mockResolvedValue({ sessions: [] });
    deleteAccount.mockResolvedValue({ status: "deleted" });

    renderPage();
    await waitFor(() =>
      expect(screen.getByTestId("delete-account-submit")).toBeDisabled(),
    );

    fireEvent.change(screen.getByTestId("delete-account-confirm-input"), {
      target: { value: "delete" },
    });
    await waitFor(() =>
      expect(screen.getByTestId("delete-account-submit")).toBeEnabled(),
    );

    fireEvent.click(screen.getByTestId("delete-account-submit"));
    await waitFor(() => expect(deleteAccount).toHaveBeenCalledWith("u1"));
    await waitFor(() =>
      expect(navigate).toHaveBeenCalledWith("/", { replace: true }),
    );
  });

  it("blocks account delete when active session present (sad)", async () => {
    getUser.mockResolvedValue(user());
    listVoices.mockResolvedValue({ voices: [] });
    listSessions.mockResolvedValue({
      sessions: [session("s1", { status: "live" })],
    });

    renderPage();
    await waitFor(() =>
      expect(screen.getByText(/End it before deleting/i)).toBeInTheDocument(),
    );
    fireEvent.change(screen.getByTestId("delete-account-confirm-input"), {
      target: { value: "delete" },
    });
    // Even with confirm typed, button stays disabled until session ends.
    expect(screen.getByTestId("delete-account-submit")).toBeDisabled();
    expect(deleteAccount).not.toHaveBeenCalled();
  });

  it("surfaces 409 from deleteAccount (server-side race)", async () => {
    getUser.mockResolvedValue(user());
    listVoices.mockResolvedValue({ voices: [] });
    listSessions.mockResolvedValue({ sessions: [] });
    deleteAccount.mockRejectedValue(
      new ApiError(409, "account_in_use", "API 409: account_in_use"),
    );

    renderPage();
    await waitFor(() =>
      expect(screen.getByTestId("delete-account-submit")).toBeDisabled(),
    );
    fireEvent.change(screen.getByTestId("delete-account-confirm-input"), {
      target: { value: "delete" },
    });
    fireEvent.click(screen.getByTestId("delete-account-submit"));
    await waitFor(() =>
      expect(screen.getByText(/End all live sessions/i)).toBeInTheDocument(),
    );
    expect(navigate).not.toHaveBeenCalled();
  });
});
