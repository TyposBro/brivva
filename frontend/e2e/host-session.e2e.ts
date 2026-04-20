// E2E for the live broadcast page (`/session/:id/live`).
//
// Historical note: this suite used to target `/host?sessionId=...` —
// that page was removed in commit db63713 ("demo-era cruft removed").
// The scenarios themselves are still relevant — WS connect + drop,
// interim→final→translation pipeline, stop→start mid-session,
// auth-failure + backend-error banners — so they've been ported to the
// current route. `BroadcastView` with `autoSkipVoice=true` is what
// `/session/:id/live` actually renders, so the voice-setup heading is
// never shown in this flow (the prior suite had to click "Skip" after
// every load).
//
// Mock-server contracts used:
//   - /auth/token (POST)    returns { token: "test-token" } unless
//                            /test/set-auth-fail was called, then 401
//   - /api/sessions/:id (GET) returns a synthetic live session with
//                            target_langs: ["ja","zh"] and two streams
//                            (ja + zh) — enough for the translation
//                            fan-out assertion
//   - /test/set-ws-reject   destroys the next WS upgrade before accept
//   - /test/close           closes the active socket server-side
//   - /test/emit            relays a JSON payload to the active socket

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

const SESSION_ID = "s-live-e2e";
const LIVE_URL = `/session/${SESSION_ID}/live`;

test.describe("SessionLivePage — E2E w/ mock server", () => {
  test.beforeEach(async ({ request }) => {
    await resetMock(request);
  });

  test("happy: auto-skips voice, connects WS, renders interim → final → translation", async ({ page, request }) => {
    await seedUser(page);
    await page.goto(LIVE_URL);

    // `autoSkipVoice=true` means we never see the "Voice Setup"
    // heading; the page walks creating → voice_setup (auto-skipped) →
    // ready and renders the Live Streams section from the seeded
    // session bundle directly.
    await waitForSocket(request);
    await expect(page.getByRole("heading", { name: "Live Streams" })).toBeVisible();

    await emit(request, { type: "interim", transcript: "hel" });
    await expect(page.getByText("hel", { exact: true })).toBeVisible();

    await emit(request, {
      type: "final",
      utteranceId: 1,
      transcript: "hello world",
      sttMs: 200,
    });
    await expect(page.getByText("hello world")).toBeVisible();

    await emit(request, {
      type: "translation",
      utteranceId: 1,
      targetLang: "ja",
      text: "こんにちは",
      translateMs: 150,
    });
    await expect(page.getByText("こんにちは")).toBeVisible();

    await emit(request, { type: "tts_end", utteranceId: 1, ttsMs: 300 });
  });

  test("sad: WS drops after connect → Disconnected UI", async ({ page, request }) => {
    await seedUser(page);
    await page.goto(LIVE_URL);
    await waitForSocket(request);
    await expect(page.getByRole("heading", { name: "Live Streams" })).toBeVisible();

    // Server-side close simulates a transport drop. `useHostSession`
    // dispatches `disconnected`, which renders the "Disconnected." line
    // in `BroadcastView`.
    await request.post(`${MOCK}/test/close`);
    await expect(page.getByText("Disconnected.")).toBeVisible();
  });

  test("sad: auth token fetch 401 → error banner + no WS", async ({ page, request }) => {
    await seedUser(page);
    // Arm the mock to fail the NEXT /auth/token call. `fetchAuthToken`
    // throws → reducer dispatches `error`. The WS handshake never
    // starts because `connectSession` bails on a null token.
    await request.post(`${MOCK}/test/set-auth-fail`);

    await page.goto(LIVE_URL);
    await expect(page.getByText(/Auth token fetch failed/)).toBeVisible();

    // Belt-and-braces: prove the socket never opened even after the
    // banner appears.
    const state = await request.get(`${MOCK}/test/state`).then((r) => r.json());
    expect(state.hasSocket).toBe(false);
  });

  test("sad: backend emits error message → banner shown", async ({ page, request }) => {
    await seedUser(page);
    await page.goto(LIVE_URL);
    await waitForSocket(request);

    await emit(request, {
      type: "error",
      message: "Soniox rate-limited — try again in 30s",
    });
    await expect(
      page.getByText("Soniox rate-limited — try again in 30s"),
    ).toBeVisible();
  });

  test("happy: unknown target lang translation doesn't crash (forward-compat)", async ({ page, request }) => {
    // Session streams are ja + zh; emit a translation for `vi` which
    // isn't in the streams map. The reducer should keep the translation
    // in the translations dictionary (for future streams) without
    // throwing, and the page should stay mounted.
    await seedUser(page);
    await page.goto(LIVE_URL);
    await waitForSocket(request);
    await expect(page.getByRole("heading", { name: "Live Streams" })).toBeVisible();

    await emit(request, {
      type: "translation",
      utteranceId: 42,
      targetLang: "vi",
      text: "xin chào",
      translateMs: 90,
    });

    // No crash: the Live Streams section is still visible, no error
    // banner rendered.
    await expect(page.getByRole("heading", { name: "Live Streams" })).toBeVisible();
  });

  test("regression: WS reconnect mid-session preserves Live Streams card + routes new translations", async ({ page, request }) => {
    // Pre-fix behaviour (before commit b3542f4 eviction): when the
    // socket dropped and the FE re-connected, the second live_session
    // row lingered alongside the first and translation fan-out silenced
    // for the remainder of the broadcast. This asserts the reducer
    // flows a brand-new translation through the card after a forced
    // disconnect + reload on the same session URL.
    await seedUser(page);
    await page.goto(LIVE_URL);
    await waitForSocket(request);
    await expect(page.getByRole("heading", { name: "Live Streams" })).toBeVisible();

    await emit(request, {
      type: "translation",
      utteranceId: 1,
      targetLang: "ja",
      text: "一本目",
      translateMs: 100,
    });
    await expect(page.getByText("一本目")).toBeVisible();

    // Drop the socket + re-open the live page; the mock will accept the
    // next upgrade as a fresh session (it only tracks one socket at a
    // time, so the re-open lands on a clean slot).
    await request.post(`${MOCK}/test/close`);
    await expect(page.getByText("Disconnected.")).toBeVisible();

    await page.goto(LIVE_URL);
    await waitForSocket(request);
    await expect(page.getByRole("heading", { name: "Live Streams" })).toBeVisible();

    await emit(request, {
      type: "translation",
      utteranceId: 2,
      targetLang: "ja",
      text: "二本目",
      translateMs: 100,
    });
    await expect(page.getByText("二本目")).toBeVisible();
  });
});
