import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { readDefaultTargetLang, writeDefaultTargetLang } from "./default-target-lang";

class FakeStorage {
  private store = new Map<string, string>();
  getItem(k: string) { return this.store.get(k) ?? null; }
  setItem(k: string, v: string) { this.store.set(k, String(v)); }
  removeItem(k: string) { this.store.delete(k); }
}

describe("default-target-lang", () => {
  beforeEach(() => {
    vi.stubGlobal("localStorage", new FakeStorage());
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("write then read round-trips (happy)", () => {
    writeDefaultTargetLang("ja");
    expect(readDefaultTargetLang()).toBe("ja");
  });

  it("returns null when nothing is persisted (edge)", () => {
    expect(readDefaultTargetLang()).toBeNull();
  });

  it("read swallows storage failure (sad)", () => {
    vi.stubGlobal("localStorage", {
      getItem: () => {
        throw new Error("blocked");
      },
      setItem: () => {},
      removeItem: () => {},
    });
    expect(readDefaultTargetLang()).toBeNull();
  });

  it("write swallows storage failure (sad)", () => {
    vi.stubGlobal("localStorage", {
      getItem: () => null,
      setItem: () => {
        throw new Error("blocked");
      },
      removeItem: () => {},
    });
    expect(() => writeDefaultTargetLang("ja")).not.toThrow();
  });
});
