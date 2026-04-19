import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, waitFor } from "@testing-library/react";

vi.mock("react-router-dom", async (orig) => {
  const actual = await orig<typeof import("react-router-dom")>();
  return {
    ...actual,
    useParams: () => ({ id: "s1" }),
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

const broadcastViewSpy = vi.fn<(p: unknown) => null>(() => null);
vi.mock("./broadcast-view", () => ({
  BroadcastView: (p: unknown) => broadcastViewSpy(p),
}));

import { signIn, _resetForTesting } from "../../../shared/auth/auth-store";
import SessionLivePage from "./session-live-page";
import { MemoryRouter } from "react-router-dom";

class FakeStorage {
  private store = new Map<string, string>();
  getItem(k: string) { return this.store.get(k) ?? null; }
  setItem(k: string, v: string) { this.store.set(k, String(v)); }
  removeItem(k: string) { this.store.delete(k); }
}

describe("SessionLivePage", () => {
  beforeEach(() => {
    _resetForTesting();
    vi.stubGlobal("localStorage", new FakeStorage());
    signIn("u1");
    getSession.mockReset();
    broadcastViewSpy.mockClear();
  });

  it("loads session, mounts BroadcastView with autoSkipVoice (happy)", async () => {
    getSession.mockResolvedValue({
      session: {
        id: "s1",
        user_id: "u1",
        voice_id: "v1",
        title: "Live test",
        source_lang: "ko",
        target_langs: '["zh"]',
        status: "live",
        live_session_id: "live-1",
        created_at: 1,
      },
      streams: [],
    });
    render(
      <MemoryRouter initialEntries={["/session/s1/live"]}>
        <SessionLivePage />
      </MemoryRouter>,
    );
    await waitFor(() => expect(broadcastViewSpy).toHaveBeenCalled());
    const props = broadcastViewSpy.mock.calls[broadcastViewSpy.mock.calls.length - 1][0] as {
      sourceLang: string;
      autoSkipVoice?: boolean;
    };
    expect(props.sourceLang).toBe("ko");
    expect(props.autoSkipVoice).toBe(true);
  });

  it("falls back to en when session lookup returns null (edge)", async () => {
    getSession.mockResolvedValue({ session: null, streams: [] });
    render(
      <MemoryRouter initialEntries={["/session/s1/live"]}>
        <SessionLivePage />
      </MemoryRouter>,
    );
    await waitFor(() => expect(broadcastViewSpy).toHaveBeenCalled());
    const props = broadcastViewSpy.mock.calls[broadcastViewSpy.mock.calls.length - 1][0] as { sourceLang: string };
    expect(props.sourceLang).toBe("en");
  });

  it("shows loader while session lookup is pending (sad)", () => {
    getSession.mockReturnValue(new Promise(() => {}));
    render(
      <MemoryRouter initialEntries={["/session/s1/live"]}>
        <SessionLivePage />
      </MemoryRouter>,
    );
    // Loader is the only element; broadcastView never mounts.
    expect(broadcastViewSpy).not.toHaveBeenCalled();
    expect(document.querySelector(".animate-spin")).toBeInTheDocument();
  });
});
