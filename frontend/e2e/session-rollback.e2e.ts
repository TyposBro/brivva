// Scenario 3 — session end + rollback.
//
// Originally targeted `/host?sessionId=...`; that page was removed in
// commit db63713 and the scenario is now wired against the current
// `/session/:id/live` route. Covers the 2am-incident graceful path:
//   1. Broadcast is live.
//   2. Server-rs kill-switch fires (voice clone broken) → error banner
//      renders in-place without tearing down the pipeline.
//   3. WS drops → "Disconnected." state visible.
//   4. Host navigates back to `/session/:id` + clicks End Session.
//   5. Summary endpoint is arranged to fail → SessionPage renders the
//      graceful-fallback message instead of a white screen.
//
// This exercises every observability + recovery surface `BroadcastView`
// is supposed to expose to the 2am operator (rollback.sh docs §2c).

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

const SESSION_ID = "s-rollback";
const LIVE_URL = `/session/${SESSION_ID}/live`;
const POST_URL = `/session/${SESSION_ID}`;

test.describe("Scenario 3 — session end + rollback", () => {
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

    await page.goto(LIVE_URL);
    await waitForSocket(request);

    // `autoSkipVoice=true` means the live page lands on the "ready"
    // state immediately once the WS handshake completes. Assert the
    // Live Streams section is the thing in view (not a Voice Setup
    // heading — that UI is only shown on the /setup route now).
    await expect(page.getByRole("heading", { name: "Live Streams" })).toBeVisible();

    // Trigger backend kill-switch event — UI shows the broadcast-scoped
    // error banner. This proves the graceful-degradation indicator
    // renders without tearing down the pipeline (the WS stays up).
    await request.post(`${MOCK}/test/fire-kill-switch`);
    await expect(page.getByText(/falling back to default voice/i)).toBeVisible();

    // Now drop the WebSocket. `useHostSession` transitions to
    // `disconnected`; BroadcastView shows the "Disconnected." line.
    await request.post(`${MOCK}/test/close`);
    await expect(page.getByText("Disconnected.")).toBeVisible();

    // Navigate to the post-live session page. Arm the summary endpoint
    // to 500 so the SessionPage hits its graceful-fallback path instead
    // of the happy-summary-modal path.
    await request.post(`${MOCK}/test/set-summary-fail`);
    await page.goto(POST_URL);
    await page.getByRole("button", { name: /End Session/i }).click();

    await expect(page.getByRole("heading", { name: /Session ended/i })).toBeVisible();
    await expect(page.getByText(/Summary unavailable/i)).toBeVisible();
  });
});
