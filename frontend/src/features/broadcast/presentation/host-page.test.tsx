import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";

const navigate = vi.fn();
vi.mock("react-router-dom", async (orig) => {
  const actual = await orig<typeof import("react-router-dom")>();
  return {
    ...actual,
    useNavigate: () => navigate,
    useSearchParams: () => [
      new URLSearchParams({ sessionId: "s1", sourceLang: "ko" }),
      vi.fn(),
    ],
    useParams: () => ({ id: "s1" }),
  };
});

const broadcastViewSpy = vi.fn<(p: unknown) => null>(() => null);
vi.mock("./broadcast-view", () => ({
  BroadcastView: (props: unknown) => broadcastViewSpy(props),
}));

import { signIn, _resetForTesting } from "../../../shared/auth/auth-store";
import HostPage from "./host-page";
import { MemoryRouter } from "react-router-dom";

class FakeStorage {
  private store = new Map<string, string>();
  getItem(k: string) { return this.store.get(k) ?? null; }
  setItem(k: string, v: string) { this.store.set(k, String(v)); }
  removeItem(k: string) { this.store.delete(k); }
}

describe("HostPage (legacy /host wrapper)", () => {
  beforeEach(() => {
    _resetForTesting();
    vi.stubGlobal("localStorage", new FakeStorage());
    broadcastViewSpy.mockClear();
  });

  it("forwards sessionId + sourceLang from query params (happy)", async () => {
    signIn("u1");
    render(
      <MemoryRouter initialEntries={["/host?sessionId=s1&sourceLang=ko"]}>
        <HostPage />
      </MemoryRouter>,
    );
    await waitFor(() => expect(broadcastViewSpy).toHaveBeenCalled());
    const args = broadcastViewSpy.mock.calls[0] as unknown as [Record<string, unknown>];
    const props = args[0] as {
      sessionId?: string;
      sourceLang: string;
      userId: string;
      autoSkipVoice?: boolean;
    };
    expect(props.sessionId).toBe("s1");
    expect(props.sourceLang).toBe("ko");
    expect(props.userId).toBe("u1");
    expect(props.autoSkipVoice).toBeFalsy();
  });

  it("renders sign-in gate when not signed in (sad)", () => {
    render(
      <MemoryRouter initialEntries={["/host"]}>
        <HostPage />
      </MemoryRouter>,
    );
    expect(screen.getByRole("button", { name: /Sign in with Google/i })).toBeInTheDocument();
    expect(broadcastViewSpy).not.toHaveBeenCalled();
  });
});
