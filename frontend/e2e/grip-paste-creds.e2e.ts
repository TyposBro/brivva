import { test, expect, type APIRequestContext, type Page } from "@playwright/test";
import { MOCK } from "./config";

// Shape-realistic Grip (AWS-IVS) values the host would paste from the Grip
// Business Center. The old scenario saved these via POST /auth/grip and
// asserted they pre-filled on reload. That path is gone: Grip stream keys
// are one-shot per broadcast (IVS rejects a duplicate publisher — the
// second session silently dies after ~25s). The FE now renders an
// ephemeral-key warning, hides Save, and filters Grip rows out of
// /api/credentials so nothing pre-fills.
const GRIP_RTMP = "rtmps://abc123def456.global-contribute.live-video.net:443/app";
const GRIP_KEY = "sk_ap-northeast-2_abcdef1234567890_examplekey";

async function resetMock(req: APIRequestContext) {
  await req.post(`${MOCK}/test/reset`);
}

async function seedOnboarded(req: APIRequestContext) {
  await req.post(`${MOCK}/test/seed-user`, {
    data: {
      user_id: "e2e-user",
      onboarding_completed_at: Math.floor(Date.now() / 1000),
      active_voice_id: "v-seed",
    },
  });
  // Give the user a voice whose source_lang matches the dashboard default
  // ("ko") so the voice-lang-mismatch banner doesn't block Go Live and
  // distract from the creds flow.
  await req.post(`${MOCK}/test/seed-voice`, {
    data: { user_id: "e2e-user", source_lang: "ko", name: "Aziz" },
  });
}

async function signIn(page: Page) {
  await page.addInitScript(() => {
    localStorage.setItem("brivva_user_id", "e2e-user");
  });
}

test.describe("Scenario 4 — Grip creds are ephemeral (no save, no pre-fill)", () => {
  test.beforeEach(async ({ request }) => {
    await resetMock(request);
    await seedOnboarded(request);
  });

  test("Grip shows warning + no Save button, and reload never pre-fills inputs", async ({ page }) => {
    await signIn(page);
    await page.goto("/dashboard");

    // Add Grip destination from the picker.
    await page.getByRole("button", { name: /Add destination/i }).click();
    await page.getByRole("button", { name: /^Grip$/i }).click();

    // Config panel auto-expands for paste-creds platforms without saved creds.
    // Paste inputs render, but the behaviour around them is different from
    // every other paste-creds platform.
    const serverUrlInput = page.getByPlaceholder("Server URL");
    const streamKeyInput = page.getByPlaceholder("Stream Key");
    await expect(serverUrlInput).toBeVisible();
    await expect(streamKeyInput).toBeVisible();

    // Amber one-shot-key warning is rendered in place of the Save button.
    await expect(page.getByTestId("grip-ephemeral-warning")).toBeVisible();
    await expect(page.getByTestId("grip-ephemeral-warning")).toContainText(/one-shot/i);

    // Save button must NOT exist for Grip.
    await expect(page.getByRole("button", { name: /Save credentials/i })).toHaveCount(0);

    // Fill the pasted values so the host can still go live this session.
    await serverUrlInput.fill(GRIP_RTMP);
    await streamKeyInput.fill(GRIP_KEY);

    // Reload → dashboard refetches /api/credentials. Because the FE filters
    // Grip rows out (and the workers endpoint returns 410 for save anyway),
    // nothing ever got stored server-side; re-adding Grip must start empty.
    await page.reload();
    await page.getByRole("button", { name: /Add destination/i }).click();
    await page.getByRole("button", { name: /^Grip$/i }).click();

    const freshServerUrl = page.getByPlaceholder("Server URL");
    const freshStreamKey = page.getByPlaceholder("Stream Key");
    await expect(freshServerUrl).toBeVisible();
    await expect(freshStreamKey).toBeVisible();
    await expect(freshServerUrl).toHaveValue("");
    await expect(freshStreamKey).toHaveValue("");

    // The "Pre-filled from saved credentials" badge must never appear for
    // Grip, even transiently.
    await expect(
      page.getByText(/Pre-filled from saved credentials/i),
    ).toHaveCount(0);
    // Warning is back on the fresh card too.
    await expect(page.getByTestId("grip-ephemeral-warning")).toBeVisible();
  });
});
