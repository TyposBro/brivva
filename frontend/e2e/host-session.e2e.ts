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
});
