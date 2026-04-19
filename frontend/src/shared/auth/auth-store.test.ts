import { describe, it, expect, beforeEach, vi, afterEach } from "vitest";
import {
  _resetForTesting,
  configureAuth,
  decodeJwtExpiry,
  ensureFreshToken,
  getCachedToken,
  getUserId,
  isSignedIn,
  loadPersistedUser,
  signIn,
  signOut,
  subscribe,
} from "./auth-store";

class FakeStorage {
  private store = new Map<string, string>();
  getItem(k: string) { return this.store.get(k) ?? null; }
  setItem(k: string, v: string) { this.store.set(k, String(v)); }
  removeItem(k: string) { this.store.delete(k); }
}

function jwt(expEpochSec: number): string {
  const header = btoa(JSON.stringify({ alg: "none" }));
  const payload = btoa(JSON.stringify({ exp: expEpochSec }));
  return `${header}.${payload}.sig`;
}

describe("auth-store", () => {
  beforeEach(() => {
    _resetForTesting();
    vi.stubGlobal("localStorage", new FakeStorage());
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-04-19T00:00:00Z"));
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  describe("decodeJwtExpiry", () => {
    it("returns exp claim in ms", () => {
      const tok = jwt(1_800_000_000);
      expect(decodeJwtExpiry(tok)).toBe(1_800_000_000_000);
    });

    it("falls back to 5 min for malformed tokens (sad)", () => {
      const ms = decodeJwtExpiry("garbage");
      expect(ms).toBe(Date.now() + 5 * 60_000);
    });
  });

  describe("signIn / signOut", () => {
    it("signIn persists user_id to localStorage but JWT only in memory", () => {
      const tok = jwt(Math.floor(Date.now() / 1000) + 600);
      signIn("u1", tok);
      expect(getUserId()).toBe("u1");
      expect(getCachedToken()).toBe(tok);
      expect(localStorage.getItem("brivva_user_id")).toBe("u1");
      expect(localStorage.getItem("brivva_jwt")).toBeNull();
    });

    it("signOut clears state + localStorage", () => {
      signIn("u1", jwt(Math.floor(Date.now() / 1000) + 600));
      signOut();
      expect(getUserId()).toBeNull();
      expect(getCachedToken()).toBeNull();
      expect(isSignedIn()).toBe(false);
      expect(localStorage.getItem("brivva_user_id")).toBeNull();
    });

    it("signIn without a token leaves cache empty (sad)", () => {
      signIn("u1");
      expect(getUserId()).toBe("u1");
      expect(getCachedToken()).toBeNull();
    });
  });

  describe("loadPersistedUser", () => {
    it("hydrates user_id from localStorage on boot", () => {
      localStorage.setItem("brivva_user_id", "stored-user");
      loadPersistedUser();
      expect(getUserId()).toBe("stored-user");
    });
  });

  describe("ensureFreshToken", () => {
    it("throws when not signed in (sad)", async () => {
      await expect(ensureFreshToken()).rejects.toThrow("Sign-in required");
    });

    it("throws when configureAuth never called (sad)", async () => {
      signIn("u1");
      await expect(ensureFreshToken()).rejects.toThrow("Auth not configured");
    });

    it("returns cached token while still fresh (happy)", async () => {
      const tok = jwt(Math.floor(Date.now() / 1000) + 600);
      const fetchToken = vi.fn().mockResolvedValue("never-called");
      configureAuth({ fetchToken });
      signIn("u1", tok);
      await expect(ensureFreshToken()).resolves.toBe(tok);
      expect(fetchToken).not.toHaveBeenCalled();
    });

    it("re-fetches when token within refresh lead window (happy)", async () => {
      const stale = jwt(Math.floor(Date.now() / 1000) + 5);
      const fresh = jwt(Math.floor(Date.now() / 1000) + 600);
      const fetchToken = vi.fn().mockResolvedValue(fresh);
      configureAuth({ fetchToken });
      signIn("u1", stale);
      await expect(ensureFreshToken()).resolves.toBe(fresh);
      expect(fetchToken).toHaveBeenCalledWith("u1");
    });

    it("dedupes concurrent calls (race)", async () => {
      const fresh = jwt(Math.floor(Date.now() / 1000) + 600);
      const fetchToken = vi.fn().mockResolvedValue(fresh);
      configureAuth({ fetchToken });
      signIn("u1");
      const [a, b] = await Promise.all([ensureFreshToken(), ensureFreshToken()]);
      expect(a).toBe(fresh);
      expect(b).toBe(fresh);
      expect(fetchToken).toHaveBeenCalledTimes(1);
    });
  });

  describe("subscribe", () => {
    it("notifies listeners on sign-in / sign-out", () => {
      const fn = vi.fn();
      const unsub = subscribe(fn);
      signIn("u1");
      expect(fn).toHaveBeenCalledTimes(1);
      signOut();
      expect(fn).toHaveBeenCalledTimes(2);
      unsub();
      signIn("u2");
      expect(fn).toHaveBeenCalledTimes(2);
    });
  });
});
