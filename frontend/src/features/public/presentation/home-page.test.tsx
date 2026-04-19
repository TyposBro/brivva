import { describe, it, expect, vi, beforeEach } from "vitest";
import { render } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";

import HomePage from "./home-page";
import {
  _resetForTesting as resetAuth,
  getCachedToken,
  getUserId,
} from "../../../shared/auth/auth-store";

// Mock useNavigate so we can assert the OAuth flow forwards to /dashboard.
const navigate = vi.fn();
vi.mock("react-router-dom", async (importOriginal) => {
  const actual = await importOriginal<typeof import("react-router-dom")>();
  return { ...actual, useNavigate: () => navigate };
});

// Stub localStorage with a Map-backed implementation so we control state
// between tests. Bun's jsdom runner ships a localStorage whose only methods
// are getItem/setItem — no removeItem/clear — which trips up normal fixtures.
class FakeStorage {
  private store = new Map<string, string>();
  get length() { return this.store.size; }
  key(i: number) { return Array.from(this.store.keys())[i] ?? null; }
  getItem(k: string) { return this.store.get(k) ?? null; }
  setItem(k: string, v: string) { this.store.set(k, String(v)); }
  removeItem(k: string) { this.store.delete(k); }
  clear() { this.store.clear(); }
}

function renderAt(path: string) {
  // MemoryRouter can't control window.location.hash for us — set it manually
  // so the OAuth-fragment parser in HomePage sees the payload we expect.
  const url = new URL(path, "https://brivva.local");
  window.history.replaceState(null, "", url.pathname + url.search + url.hash);
  return render(
    <MemoryRouter>
      <HomePage />
    </MemoryRouter>,
  );
}

describe("HomePage OAuth callback", () => {
  beforeEach(() => {
    vi.stubGlobal("localStorage", new FakeStorage());
    navigate.mockReset();
    resetAuth();
  });

  it("does not touch auth state or navigate on a clean landing (happy)", () => {
    renderAt("/");
    expect(getUserId()).toBeNull();
    expect(getCachedToken()).toBeNull();
    expect(navigate).not.toHaveBeenCalled();
  });

  it("persists user_id + JWT in memory and navigates to /dashboard (happy)", () => {
    renderAt("/?user_id=callback-user#token=fake.jwt.token");
    // user_id survives reloads (localStorage); JWT lives only in memory.
    expect(localStorage.getItem("brivva_user_id")).toBe("callback-user");
    expect(localStorage.getItem("brivva_jwt")).toBeNull();
    expect(getUserId()).toBe("callback-user");
    expect(getCachedToken()).toBe("fake.jwt.token");
    expect(navigate).toHaveBeenCalledWith("/dashboard", { replace: true });
  });

  it("persists user_id alone and navigates without a token fragment (edge)", () => {
    renderAt("/?user_id=only-user");
    expect(localStorage.getItem("brivva_user_id")).toBe("only-user");
    expect(getUserId()).toBe("only-user");
    expect(getCachedToken()).toBeNull();
    // user_id-only landings still forward to the dashboard so the gated UI
    // can decide whether to demand a fresh OAuth round-trip.
    expect(navigate).toHaveBeenCalledWith("/dashboard", { replace: true });
  });

  it("clears the token fragment from the URL so a refresh doesn't re-persist it (edge)", () => {
    renderAt("/?user_id=u1#token=abc");
    expect(window.location.hash).toBe("");
    expect(window.location.search).toBe("");
  });

  it("ignores a fragment that carries no token= key (sad)", () => {
    renderAt("/?user_id=u1#something_else=yes");
    expect(getUserId()).toBe("u1");
    expect(getCachedToken()).toBeNull();
    expect(navigate).toHaveBeenCalledWith("/dashboard", { replace: true });
  });
});
