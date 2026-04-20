import { test, expect, type Page, type APIRequestContext } from "@playwright/test";
import { MOCK } from "./config";

async function resetMock(req: APIRequestContext) {
  await req.post(`${MOCK}/test/reset`);
}

async function waitForSocket(req: APIRequestContext, timeoutMs = 5000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const r = await req.get(`${MOCK}/test/state`);
    const body = await r.json();
    if (body.hasSocket) return body;
    await new Promise((r) => setTimeout(r, 50));
  }
  throw new Error("WS never connected to mock server");
}

async function emit(req: APIRequestContext, payload: unknown) {
  const r = await req.post(`${MOCK}/test/emit`, { data: payload });
  if (!r.ok()) throw new Error(`emit failed: ${r.status()}`);
}

async function seedUser(page: Page) {
  await page.addInitScript(() => {
    localStorage.setItem("brivva_user_id", "e2e-user");
  });
}

test.describe("HostPage — E2E w/ mock server", () => {
  test.beforeEach(async ({ request }) => { await resetMock(request); });

  test("happy: loads session, skips voice, records, sees interim → final → translation", async ({ page, request }) => {
    await seedUser(page);
    await page.goto("/host?sessionId=s1&sourceLang=en");

    await expect(page.getByRole("heading", { name: "Voice Setup" })).toBeVisible();
    await waitForSocket(request);

    await page.getByRole("button", { name: /Skip/ }).click();
    await expect(page.getByRole("heading", { name: "Live Streams" })).toBeVisible();

    await emit(request, { type: "interim", transcript: "hel" });
    await expect(page.getByText("hel", { exact: true })).toBeVisible();

    await emit(request, { type: "final", utteranceId: 1, transcript: "hello world", sttMs: 200 });
    await expect(page.getByText("hello world")).toBeVisible();

    await emit(request, { type: "translation", utteranceId: 1, targetLang: "ja", text: "こんにちは", translateMs: 150 });
    await expect(page.getByText("こんにちは")).toBeVisible();

    await emit(request, { type: "tts_end", utteranceId: 1, ttsMs: 300 });
  });

  test("sad: WS drops after connect → Disconnected UI", async ({ page, request }) => {
    await seedUser(page);
    await page.goto("/host?sessionId=s1&sourceLang=en");
    await waitForSocket(request);
    await expect(page.getByRole("heading", { name: "Voice Setup" })).toBeVisible();

    await request.post(`${MOCK}/test/close`);
    await expect(page.getByText(/Disconnected/i)).toBeVisible();
  });

  test("sad: auth token fetch 401 → error banner", async ({ page, request }) => {
    await seedUser(page);
    await request.post(`${MOCK}/test/set-auth-fail`);
    await page.goto("/host?sessionId=s1&sourceLang=en");

    await expect(page.getByText(/Auth token fetch failed/i)).toBeVisible({ timeout: 10_000 });
    await expect(page.getByRole("heading", { name: "Voice Setup" })).not.toBeVisible();
  });

  test("sad: backend emits error message → banner shown", async ({ page, request }) => {
    await seedUser(page);
    await page.goto("/host?sessionId=s1&sourceLang=en");
    await waitForSocket(request);

    await emit(request, { type: "error", message: "backend exploded" });
    await expect(page.getByText("backend exploded")).toBeVisible();
  });

  test("happy: unknown target lang translation doesn't crash (forward-compat)", async ({ page, request }) => {
    await seedUser(page);
    await page.goto("/host?sessionId=s1&sourceLang=en");
    await waitForSocket(request);
    await page.getByRole("button", { name: /Skip/ }).click();

    await emit(request, { type: "final", utteranceId: 7, transcript: "foo" });
    await emit(request, { type: "translation", utteranceId: 7, targetLang: "ru", text: "привет" });
    // Page still mounted; stream cards for ja/zh unchanged
    await expect(page.getByRole("heading", { name: "Live Streams" })).toBeVisible();
    await expect(page.getByText("foo")).toBeVisible();
  });

  // Production regression (2026-04-20): host pressed stop -> start without
  // ending the session. Server-side eviction now guarantees only one
  // live_session per FE session_id; the FE must keep the WS open and
  // continue rendering translations after the second start.
  test("regression: stop -> start within session keeps Live Streams + translations flowing", async ({ page, request }) => {
    await seedUser(page);
    await page.goto("/host?sessionId=s1&sourceLang=en");
    await waitForSocket(request);
    await page.getByRole("button", { name: /Skip/ }).click();
    await expect(page.getByRole("heading", { name: "Live Streams" })).toBeVisible();

    await page.getByRole("button", { name: /^Record$/ }).click();
    await emit(request, { type: "final", utteranceId: 1, transcript: "first run", sttMs: 100 });
    await expect(page.getByText("first run")).toBeVisible();
    await emit(request, { type: "translation", utteranceId: 1, targetLang: "ja", text: "最初", translateMs: 80 });
    await expect(page.getByText("最初")).toBeVisible();
    await page.getByRole("button", { name: /^Stop$/ }).click();

    // Second recording window — start again WITHOUT ending the session.
    // Pre-fix this is where TTS went silent on the server because the
    // prior live_session lingered. The FE state must stay healthy: Live
    // Streams visible, no error banner, and a fresh translation renders.
    await page.getByRole("button", { name: /^Record$/ }).click();
    await emit(request, { type: "final", utteranceId: 2, transcript: "second run", sttMs: 110 });
    await expect(page.getByText("second run")).toBeVisible();
    await emit(request, { type: "translation", utteranceId: 2, targetLang: "ja", text: "二回目", translateMs: 90 });
    await expect(page.getByText("二回目")).toBeVisible();
    await expect(page.getByRole("heading", { name: "Live Streams" })).toBeVisible();
  });

  // Regression — prod incident 2026-04-20. Clicking End-Session used to flash
  // "Session not found" because Workers hard-deleted the row, the FE refresh
  // GET returned `session:null`, and the page short-circuited to its
  // "missing" branch instead of opening the summary modal. Soft-end + an
  // optimistic state flip on End should mean the host only ever sees the
  // summary surface.
  test("regression: End Session shows summary modal — never the 'Session not found' fallback", async ({ page }) => {
    await seedUser(page);
    await page.goto("/session/s-end-test");

    // Wait for the live session to render so the End button is reachable.
    await expect(page.getByText("E2E Test Session")).toBeVisible();
    await page.getByRole("button", { name: /End Session/i }).click();

    await expect(page.getByRole("heading", { name: /Session ended/i })).toBeVisible();
    // The fallback state must not appear at any point during/after End.
    await expect(page.getByText(/Session not found/i)).toHaveCount(0);
  });
});
