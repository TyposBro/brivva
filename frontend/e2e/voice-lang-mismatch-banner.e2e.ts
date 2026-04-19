import { test, expect, type APIRequestContext, type Page } from "@playwright/test";
import { MOCK } from "./config";

async function resetMock(req: APIRequestContext) {
  await req.post(`${MOCK}/test/reset`);
}

// Seed an onboarded user whose voice clone was enrolled in English. The
// dashboard defaults the session source to Korean, so the page will boot
// directly into the mismatch state and the banner should render on first
// paint — no user interaction required to surface it.
async function seedUserWithEnglishVoice(req: APIRequestContext) {
  await req.post(`${MOCK}/test/seed-user`, {
    data: {
      user_id: "e2e-user",
      onboarding_completed_at: Math.floor(Date.now() / 1000),
      active_voice_id: "v-seed",
      youtube_connected: true,
      youtube_channel_name: "Aziz",
    },
  });
  await req.post(`${MOCK}/test/seed-voice`, {
    data: {
      user_id: "e2e-user",
      id: "v-seed",
      source_lang: "en",
      name: "Aziz",
    },
  });
}

async function signIn(page: Page) {
  await page.addInitScript(() => {
    localStorage.setItem("brivva_user_id", "e2e-user");
  });
}

test.describe("Scenario 6 — voice/session source-lang mismatch banner", () => {
  test.beforeEach(async ({ request }) => {
    await resetMock(request);
    await seedUserWithEnglishVoice(request);
  });

  test("banner appears on mismatch, blocks Go Live, CTAs resolve it both ways", async ({ page }) => {
    await signIn(page);
    await page.goto("/dashboard");

    // The dashboard's session-source <select> defaults to ko; the seeded
    // voice is en → mismatch is immediate.
    const banner = page.getByTestId("voice-lang-mismatch-banner");
    await expect(banner).toBeVisible();
    await expect(banner).toContainText(/English/);
    await expect(banner).toContainText(/Korean/);

    // Two CTAs inside the banner: "Re-record voice" + a language-swap
    // button. Both should be present.
    await expect(banner.getByRole("button", { name: /Re-record voice/i })).toBeVisible();
    const swapCta = banner.getByRole("button", {
      name: /Change session source to English/i,
    });
    await expect(swapCta).toBeVisible();

    // Add a destination so the Go Live button advances past the
    // "Add a destination..." label and we can assert the mismatch-specific
    // copy. Local Test needs no creds.
    await page.getByRole("button", { name: /Add destination/i }).click();
    await page.getByRole("button", { name: /Local Test/i }).click();

    // Go Live label reflects the mismatch-gate; button is disabled.
    const goLive = page.getByRole("button", { name: /Fix voice language to go live/i });
    await expect(goLive).toBeVisible();
    await expect(goLive).toBeDisabled();

    // Swap CTA flips session source_lang=en → banner disappears, Go Live
    // re-enables. Note: adding a Local Test destination defaulted its own
    // dest-lang to en (pickDestinationLang falls back when session=ko), so
    // after the swap dest.lang === sourceLang === "en" → destinationError
    // fires with "English is your source language". Remove the destination
    // to isolate the mismatch-resolution assertion.
    await swapCta.click();
    await expect(banner).toHaveCount(0);
    // The language select that now reads "en" is the SESSION picker (first
    // combobox). Verify the switch happened.
    await expect(page.getByRole("combobox").first()).toHaveValue("en");

    // Flip the session source back to ko → banner re-appears.
    await page.getByRole("combobox").first().selectOption("ko");
    await expect(page.getByTestId("voice-lang-mismatch-banner")).toBeVisible();
  });

  test("Re-record CTA expands voice section and seeds picker with session source-lang", async ({ page }) => {
    await signIn(page);
    await page.goto("/dashboard");

    const banner = page.getByTestId("voice-lang-mismatch-banner");
    await expect(banner).toBeVisible();

    // Session source is ko; clicking Re-record opens the settings drawer,
    // then imperatively enters YourVoiceSection's recording mode. The
    // section's useEffect hook watches `recordRequestKey`, so bumping it
    // seeds the picker from `initialSourceLang` (= the session's current
    // source_lang, which is "ko" here).
    //
    // Observed quirk: YourVoiceSection's `firstKey` ref initialises to the
    // prop value on mount, so the very first bump is a no-op when the
    // drawer had not previously rendered the section. Clicking the CTA a
    // second time (drawer already open, section already mounted at key=1)
    // delivers a real bump → recording opens with the picker seeded to
    // "ko". Documenting this is the spirit of Task C step 9.
    const rerecord = banner.getByRole("button", { name: /Re-record voice/i });
    await rerecord.click();
    // Settings drawer is now visible (YouTube Account label lives inside it).
    await expect(page.getByText(/YouTube Account/i)).toBeVisible();
    await rerecord.click();

    // YourVoiceSection's recording mode surfaces the SourceLangPicker with a
    // dashboard-specific label.
    const picker = page.locator("#dashboard-source-lang");
    await expect(picker).toBeVisible();

    // Korean radio is selected (aria-checked=true) — matches the session's
    // source_lang. The English radio (the voice's enrollment language) is
    // NOT selected because the re-record step exists to change it.
    await expect(picker.getByRole("radio", { name: /Korean/i })).toHaveAttribute(
      "aria-checked",
      "true",
    );
    await expect(picker.getByRole("radio", { name: /English/i })).toHaveAttribute(
      "aria-checked",
      "false",
    );

    // The re-record header is visible too.
    await expect(
      page.getByRole("heading", { name: /Voice Setup/i }),
    ).toBeVisible();
  });
});
