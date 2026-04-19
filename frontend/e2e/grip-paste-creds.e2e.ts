import { test, expect, type APIRequestContext, type Page } from "@playwright/test";
import { MOCK } from "./config";

// Shape-realistic Grip (AWS-IVS) values the host would paste from the Grip
// Business Center. The mock server echoes them back via /api/credentials,
// which lets us assert the "Pre-filled from saved credentials" badge on
// reload without touching a real Grip endpoint.
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

test.describe("Scenario 4 — Grip paste-creds persistence", () => {
  test.beforeEach(async ({ request }) => {
    await resetMock(request);
    await seedOnboarded(request);
  });

  test("save Grip creds → badge flips to 'Saved' → reload pre-fills from storage", async ({ page, request }) => {
    await signIn(page);
    await page.goto("/dashboard");

    // Add Grip destination from the picker.
    await page.getByRole("button", { name: /Add destination/i }).click();
    await page.getByRole("button", { name: /^Grip$/i }).click();

    // Config panel auto-expands for paste-creds platforms without saved creds.
    // Fill Server URL + Stream Key with AWS-IVS-shaped values.
    const serverUrlInput = page.getByPlaceholder("Server URL");
    const streamKeyInput = page.getByPlaceholder("Stream Key");
    await expect(serverUrlInput).toBeVisible();
    await expect(streamKeyInput).toBeVisible();

    await serverUrlInput.fill(GRIP_RTMP);
    await streamKeyInput.fill(GRIP_KEY);

    // Fire the save. POST /auth/grip persists into mock-server's in-memory
    // credentials map and the UI flips to the just-saved indicator.
    const savePromise = page.waitForResponse(
      (r) => r.url().includes("/auth/grip") && r.request().method() === "POST",
    );
    await page.getByRole("button", { name: /Save credentials/i }).click();
    const saveRes = await savePromise;
    expect(saveRes.status()).toBe(200);

    await expect(
      page.getByText(/Saved — will pre-fill next session/i),
    ).toBeVisible();

    // Sanity: mock stored the creds under the user_id.
    const credRes = await request.get(`${MOCK}/api/credentials?user_id=e2e-user`);
    const credBody = (await credRes.json()) as {
      credentials: Array<{ platform: string; rtmp_url: string; stream_key: string }>;
    };
    expect(credBody.credentials).toHaveLength(1);
    expect(credBody.credentials[0].platform).toBe("grip");
    expect(credBody.credentials[0].rtmp_url).toBe(GRIP_RTMP);
    expect(credBody.credentials[0].stream_key).toBe(GRIP_KEY);

    // Reload → dashboard fetches /api/credentials → adding Grip again should
    // pre-fill the inputs from savedCreds. With saved creds present the card
    // mounts collapsed; the config panel only renders when expanded, so the
    // "Pre-filled from saved credentials" badge is gated on clicking the
    // chevron. Assert the stored values flow through.
    await page.reload();
    await page.getByRole("button", { name: /Add destination/i }).click();
    await page.getByRole("button", { name: /^Grip$/i }).click();

    // Expand the Grip card's config panel. The chevron toggle uses the
    // lucide ChevronRight icon when collapsed, which lucide-react renders
    // with class `lucide-chevron-right`. The X remove button sits next to
    // it; we scope to the first matching button within the Grip card.
    const gripCard = page
      .locator("text=/^Grip$/")
      .first()
      .locator("xpath=ancestor::div[contains(@class,'bg-surface-container-low')][1]");
    await gripCard.locator("button:has(svg.lucide-chevron-right)").first().click();

    await expect(
      page.getByText(/Pre-filled from saved credentials/i),
    ).toBeVisible();
    await expect(page.getByPlaceholder("Server URL")).toHaveValue(GRIP_RTMP);
    await expect(page.getByPlaceholder("Stream Key")).toHaveValue(GRIP_KEY);
  });
});
