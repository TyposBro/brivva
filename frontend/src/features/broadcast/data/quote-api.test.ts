import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { fetchSessionQuote, fetchSessionSummary } from "./quote-api";

const ok = (body: unknown) =>
  Promise.resolve(
    new Response(JSON.stringify(body), {
      status: 200,
      headers: { "Content-Type": "application/json" },
    }),
  );

describe("fetchSessionQuote", () => {
  let fetchMock: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    fetchMock = vi.fn();
    vi.stubGlobal("fetch", fetchMock);
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("happy: maps snake_case payload + breakdown into camelCase", async () => {
    fetchMock.mockReturnValue(
      ok({
        estimated_cost_usd: 12.5,
        expected_minutes: 60,
        breakdown: [
          { lang: "ja", minutes: 30, cost_usd: 6 },
          { lang: "zh", minutes: 30, cost_usd: 6.5 },
        ],
      }),
    );
    const r = await fetchSessionQuote("s1", 60);
    expect(r.estimatedCostUsd).toBe(12.5);
    expect(r.expectedMinutes).toBe(60);
    expect(r.breakdown).toEqual([
      { lang: "ja", minutes: 30, costUsd: 6 },
      { lang: "zh", minutes: 30, costUsd: 6.5 },
    ]);
  });

  it("falls back to caller-supplied minutes when server omits expected_minutes", async () => {
    fetchMock.mockReturnValue(ok({ estimated_cost_usd: 0 }));
    const r = await fetchSessionQuote("s1", 45);
    expect(r.expectedMinutes).toBe(45);
    expect(r.breakdown).toEqual([]);
  });

  it("maps missing estimated_cost_usd to null instead of crashing (edge)", async () => {
    // Reproduces the production bug: server responds 200 but the cost field
    // is absent. Prior behaviour was a ZodError whose JSON-stringified
    // issues blob leaked into the modal. Now: soft fallback to null.
    fetchMock.mockReturnValue(ok({ expected_minutes: 30 }));
    const r = await fetchSessionQuote("s1", 30);
    expect(r.estimatedCostUsd).toBeNull();
    expect(r.expectedMinutes).toBe(30);
  });

  it("maps explicit null estimated_cost_usd to null (edge)", async () => {
    fetchMock.mockReturnValue(ok({ estimated_cost_usd: null }));
    const r = await fetchSessionQuote("s1", 30);
    expect(r.estimatedCostUsd).toBeNull();
  });

  it("still throws for unrelated schema violations (sad — don't swallow)", async () => {
    // Narrow fallback: only estimated_cost_usd is tolerated. A totally
    // wrong shape (e.g. non-array breakdown) must still raise so bugs
    // elsewhere stay visible.
    fetchMock.mockReturnValue(
      ok({ estimated_cost_usd: 1, breakdown: "not-an-array" }),
    );
    await expect(fetchSessionQuote("s1", 30)).rejects.toThrow();
  });

  it("rejects on non-2xx (sad)", async () => {
    fetchMock.mockReturnValue(
      Promise.resolve(new Response("err", { status: 503 })),
    );
    await expect(fetchSessionQuote("s1", 30)).rejects.toThrow(
      /fetchSessionQuote: 503/,
    );
  });
});

describe("fetchSessionSummary", () => {
  let fetchMock: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    fetchMock = vi.fn();
    vi.stubGlobal("fetch", fetchMock);
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("happy: maps snake_case payload into camelCase", async () => {
    fetchMock.mockReturnValue(
      ok({
        total_minutes: 63,
        total_cost_usd: 189,
        breakdown: [{ lang: "zh", minutes: 63, cost_usd: 94.5 }],
      }),
    );
    const r = await fetchSessionSummary("s1");
    expect(r.totalMinutes).toBe(63);
    expect(r.totalCostUsd).toBe(189);
    expect(r.breakdown).toEqual([{ lang: "zh", minutes: 63, costUsd: 94.5 }]);
  });

  it("works without breakdown (edge)", async () => {
    fetchMock.mockReturnValue(ok({ total_minutes: 0, total_cost_usd: 0 }));
    const r = await fetchSessionSummary("s1");
    expect(r.breakdown).toEqual([]);
  });

  it("rejects on non-2xx (sad)", async () => {
    fetchMock.mockReturnValue(
      Promise.resolve(new Response("err", { status: 404 })),
    );
    await expect(fetchSessionSummary("s1")).rejects.toThrow(
      /fetchSessionSummary: 404/,
    );
  });
});
