import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { SignInGate } from "./sign-in-gate";
import { _resetForTesting, signIn } from "./auth-store";

class FakeStorage {
  private store = new Map<string, string>();
  getItem(k: string) { return this.store.get(k) ?? null; }
  setItem(k: string, v: string) { this.store.set(k, String(v)); }
  removeItem(k: string) { this.store.delete(k); }
}

describe("SignInGate", () => {
  beforeEach(() => {
    _resetForTesting();
    vi.stubGlobal("localStorage", new FakeStorage());
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("renders children when the user is signed in (happy)", () => {
    signIn("u1");
    render(
      <SignInGate>
        <div>guarded content</div>
      </SignInGate>,
    );
    expect(screen.getByText("guarded content")).toBeInTheDocument();
  });

  it("renders the sign-in card when no user is set (sad)", () => {
    render(
      <SignInGate>
        <div>guarded content</div>
      </SignInGate>,
    );
    expect(screen.queryByText("guarded content")).not.toBeInTheDocument();
    expect(screen.getByRole("heading", { name: /Sign in to Brivva/i })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Sign in with Google/i })).toBeInTheDocument();
  });

  it("redirects the browser to /auth/google when the button is clicked", async () => {
    const assign = vi.fn();
    vi.stubGlobal("location", { ...window.location, assign });
    const user = userEvent.setup();
    render(
      <SignInGate>
        <div>guarded content</div>
      </SignInGate>,
    );
    await user.click(screen.getByRole("button", { name: /Sign in with Google/i }));
    expect(assign).toHaveBeenCalledWith(expect.stringMatching(/\/auth\/google$/));
  });
});
