import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { completeOnboarding, fetchOnboardingState } from "./onboarding-api";

const ok = (body: unknown) =>
  Promise.resolve(
    new Response(JSON.stringify(body), {
      status: 200,
      headers: { "Content-Type": "application/json" },
    }),
  );

const fail = (status: number) =>
  Promise.resolve(new Response(JSON.stringify({ error: "boom" }), { status }));

describe("onboarding-api", () => {
  let fetchMock: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    fetchMock = vi.fn();
    vi.stubGlobal("fetch", fetchMock);
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("fetchOnboardingState returns the timestamp when present (happy)", async () => {
    fetchMock.mockReturnValue(ok({ onboarding_completed_at: 1_700_000_000 }));
    await expect(fetchOnboardingState("u1")).resolves.toEqual({
      onboardingCompletedAt: 1_700_000_000,
    });
    expect(fetchMock).toHaveBeenCalledWith(
      expect.stringContaining("/api/user?user_id=u1"),
      expect.any(Object),
    );
  });

  it("fetchOnboardingState normalises missing field to null", async () => {
    fetchMock.mockReturnValue(ok({}));
    await expect(fetchOnboardingState("u1")).resolves.toEqual({
      onboardingCompletedAt: null,
    });
  });

  it("fetchOnboardingState rejects on non-2xx (sad)", async () => {
    fetchMock.mockReturnValue(fail(500));
    await expect(fetchOnboardingState("u1")).rejects.toThrow(
      /fetchOnboardingState: 500/,
    );
  });

  it("completeOnboarding POSTs the snake_case body (happy)", async () => {
    fetchMock.mockReturnValue(ok({}));
    await completeOnboarding({ userId: "u1", defaultTargetLang: "ja" });
    const body = JSON.parse(fetchMock.mock.calls[0][1].body);
    expect(body).toEqual({ user_id: "u1", default_target_lang: "ja" });
  });

  it("completeOnboarding rejects on non-2xx (sad)", async () => {
    fetchMock.mockReturnValue(fail(400));
    await expect(
      completeOnboarding({ userId: "u1", defaultTargetLang: "ja" }),
    ).rejects.toThrow(/completeOnboarding: 400/);
  });
});
