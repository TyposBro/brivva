// TODO(post-may10, demo-era-rewrite): this scenario was wired against
// the deleted `/host?sessionId=...&sourceLang=...` page (commit db63713).
// The WS-drop → kill-switch → summary-fallback path is still worth
// covering end-to-end, but it needs to be reauthored against
// `/session/:id/live` (see session-live-page.tsx). Skipped until then —
// go-live.e2e.ts covers the happy summary-modal path today.

import { test, expect, type APIRequestContext, type Page } from "@playwright/test";
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

async function signIn(page: Page) {
  await page.addInitScript(() => {
    localStorage.setItem("brivva_user_id", "e2e-user");
  });
}

test.describe.skip("Scenario 3 — session end + rollback (DISABLED — /host page removed; see TODO at top)", () => {
  test.beforeEach(async ({ request }) => {
    await resetMock(request);
    await request.post(`${MOCK}/test/seed-user`, {
      data: {
        user_id: "e2e-user",
        onboarding_completed_at: Math.floor(Date.now() / 1000),
        active_voice_id: "v-default",
      },
    });
  });

  test("WS drop → disconnect banner → kill-switch error → graceful end with summary fallback", async ({ page, request }) => {
    await signIn(page);

    await page.goto("/host?sessionId=s-rollback&sourceLang=en");
    await waitForSocket(request);

    // Skip voice → ready state.
    await expect(page.getByRole("heading", { name: /Voice Setup/i })).toBeVisible();
    await page.getByRole("button", { name: /Skip/i }).click();

    // Trigger backend kill-switch event — UI shows error banner from
    // server-rs ("falling back to default voice"). This proves the
    // graceful-degradation indicator renders.
    await request.post(`${MOCK}/test/fire-kill-switch`);
    await expect(page.getByText(/falling back to default voice/i)).toBeVisible();

    // Now drop the WebSocket. UI must transition to the "Disconnected" state.
    await request.post(`${MOCK}/test/close`);
    await expect(page.getByText(/Disconnected/i)).toBeVisible();

    // Navigate to /session/:id, end cleanly, summary endpoint fails →
    // modal still renders with the graceful "summary unavailable" message.
    await request.post(`${MOCK}/test/set-summary-fail`);
    await page.goto("/session/s-rollback");
    await page.getByRole("button", { name: /End Session/i }).click();

    await expect(page.getByRole("heading", { name: /Session ended/i })).toBeVisible();
    await expect(page.getByText(/Summary unavailable/i)).toBeVisible();
  });
});
