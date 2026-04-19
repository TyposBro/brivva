import { test, expect, type APIRequestContext, type Page } from "@playwright/test";

const MOCK = "http://localhost:8787";

async function resetMock(req: APIRequestContext) {
  await req.post(`${MOCK}/test/reset`);
}

async function seedExistingUser(req: APIRequestContext) {
  // Mark onboarding done + attach a voice + a connected YouTube channel so
  // the dashboard renders without redirecting to /onboarding.
  await req.post(`${MOCK}/test/seed-user`, {
    data: {
      user_id: "e2e-user",
      youtube_connected: true,
      youtube_channel_name: "Aziz",
      onboarding_completed_at: Math.floor(Date.now() / 1000),
      active_voice_id: "v-default",
    },
  });
  await req.post(`${MOCK}/api/voices`, {
    data: { user_id: "e2e-user", name: "Aziz", audio_base64: "x" },
  });
}

async function signIn(page: Page) {
  // Bypass the SignInGate by seeding the user_id in localStorage. The auth
  // store hydrates from there on boot.
  await page.addInitScript(() => {
    localStorage.setItem("brivva_user_id", "e2e-user");
  });
}

test.describe("Scenario 2 — existing user go-live", () => {
  test.beforeEach(async ({ request }) => {
    await resetMock(request);
    await seedExistingUser(request);
  });

  test("create session → quote modal at 60 min → /setup → /live → end → summary modal", async ({ page }) => {
    await signIn(page);
    await page.goto("/dashboard");

    // Add the local-test platform from the picker (simplest, no creds needed).
    await page.getByRole("button", { name: /Add destination/i }).click();
    await page.getByRole("button", { name: /Other/i }).click();
    await page.getByRole("button", { name: /Local Test/i }).click();

    // Title + Go Live.
    await page.getByPlaceholder("Session title").fill("E2E live test");
    await page.getByRole("button", { name: /Go Live/i }).click();

    // Quote modal pops with default 30 min — slide to 60.
    await expect(page.getByRole("heading", { name: /How long will you stream/i })).toBeVisible();
    const slider = page.getByRole("slider").first();
    await slider.evaluate((el, value) => {
      const input = el as HTMLInputElement;
      Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, "value")?.set?.call(input, String(value));
      input.dispatchEvent(new Event("input", { bubbles: true }));
      input.dispatchEvent(new Event("change", { bubbles: true }));
    }, 60);
    // Cost recomputes — 60 × $1.50 = $90.
    await expect(page.getByText(/\$90\.00/).first()).toBeVisible();

    await page.getByRole("button", { name: /^Continue$/i }).click();

    // Setup page (voice already cloned → Go Live enabled).
    await page.waitForURL(/\/setup$/);
    await page.getByRole("button", { name: /Go Live/i }).click();

    // Live page mounts; broadcast view connects to mock WS.
    await page.waitForURL(/\/live$/);

    // Navigate back to /session/:id to fire End → summary modal.
    const url = new URL(page.url());
    const sessionId = url.pathname.split("/")[2];
    await page.goto(`/session/${sessionId}`);

    await page.getByRole("button", { name: /End Session/i }).click();
    await expect(page.getByRole("heading", { name: /Session ended/i })).toBeVisible();
    await expect(page.getByText("12m", { exact: true })).toBeVisible();
    await expect(page.getByText("$18.00", { exact: true })).toBeVisible();
  });
});
