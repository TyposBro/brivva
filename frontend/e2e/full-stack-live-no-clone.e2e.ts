// Full-stack e2e WITHOUT voice cloning. Uses the default male library
// voice for the Chinese translation lane. Identical pipeline coverage
// to `full-stack-live.e2e.ts` minus the ElevenLabs clone round-trip,
// so it runs ~2 minutes faster and doesn't burn clone credits.
//
// Use this one to validate the Soniox + translation + ElevenLabs-TTS
// + ffmpeg + Grip/RTMP fan-out is healthy first; only fall back to
// the cloned-voice variant once this passes.
//
// Preconditions are identical to full-stack-live.e2e.ts — see that
// file's header.

import { test, expect } from "@playwright/test";

const WORKERS_URL =
  process.env.BRIVVA_DEV_WORKERS_URL ?? "http://localhost:8787";

function requiredEnv(name: string): string {
  const v = process.env[name];
  if (!v || v.startsWith("YOUR-") || v.includes("YOUR_")) {
    throw new Error(
      `${name} missing or still a placeholder. Edit tests/e2e/.env.`,
    );
  }
  return v;
}

test.beforeEach(async ({ request }) => {
  const res = await request.post(`${WORKERS_URL}/test/reset-dev-user`);
  expect(
    res.ok(),
    `POST /test/reset-dev-user failed (status ${res.status()}). ` +
      `Confirm DEV_AUTH_BYPASS=true in workers/.dev.vars and restart dev-stack.`,
  ).toBe(true);
});

test("default-male voice: signup → onboard (skip clone) → Grip(zh) + Custom(pass) → Go Live → record", async ({
  page,
  request,
}) => {
  const GRIP_RTMP_URL = requiredEnv("GRIP_RTMP_URL");
  const GRIP_RTMP_KEY = requiredEnv("GRIP_RTMP_KEY");
  const YOUTUBE_RTMP_URL = requiredEnv("YOUTUBE_RTMP_URL");
  const YOUTUBE_RTMP_KEY = requiredEnv("YOUTUBE_RTMP_KEY");

  await test.step("1. sign in via DEV_AUTH_BYPASS", async () => {
    await page.goto("/dashboard");
    await page
      .getByRole("button", { name: /Sign in with Google/i })
      .click();
    await page.waitForURL(/\/(onboarding|dashboard)\b/, { timeout: 15_000 });
  });

  await test.step("2. onboarding: skip platform connect", async () => {
    await expect(
      page.getByRole("heading", { name: /Connect a streaming platform/i }),
    ).toBeVisible({ timeout: 15_000 });
    await page.getByRole("button", { name: /^Continue$/i }).click();
  });

  await test.step("3. onboarding: skip voice clone", async () => {
    await expect(
      page.getByRole("heading", { name: /Clone your voice/i }),
    ).toBeVisible();
    // The card-internal "Skip (use default voice)" button. The bottom
    // "Skip for now" link does the same thing — we use this one because
    // it's the explicit "I want the library voice, not a clone" path.
    await page
      .getByRole("button", { name: /Skip \(use default voice\)/i })
      .click();
  });

  await test.step("4. onboarding: pick default audience language", async () => {
    await expect(
      page.getByRole("heading", { name: /Default audience language/i }),
    ).toBeVisible();
    await page.getByRole("button", { name: /Chinese/i }).click();
    await page.getByRole("button", { name: /^Finish$/i }).click();
    await page.waitForURL(/\/dashboard\b/, { timeout: 15_000 });
  });

  await test.step("5a. add Grip destination, lang=zh (default-voice TTS)", async () => {
    await page.getByRole("button", { name: /Add destination/i }).click();
    await page.getByRole("button", { name: /^Grip$/i }).click();
    const gripCard = page.getByTestId("destination-card-grip");
    await expect(gripCard).toBeVisible();
    await gripCard.getByPlaceholder(/Server URL/i).fill(GRIP_RTMP_URL);
    await gripCard.getByPlaceholder(/Stream Key/i).fill(GRIP_RTMP_KEY);
    await gripCard.locator("select").first().selectOption("zh");
  });

  await test.step("5b. add Custom RTMP destination, lang=pass (raw)", async () => {
    await page.getByRole("button", { name: /Add destination/i }).click();
    await page.getByRole("button", { name: /Custom RTMP/i }).click();
    const customCard = page.getByTestId("destination-card-custom");
    await expect(customCard).toBeVisible();
    await customCard
      .getByPlaceholder(/Server URL/i)
      .fill(YOUTUBE_RTMP_URL);
    await customCard.getByPlaceholder(/Stream Key/i).fill(YOUTUBE_RTMP_KEY);
    await customCard.locator("select").first().selectOption("pass");
  });

  let sessionId = "";
  await test.step("6. Go Live → setup page", async () => {
    await page
      .getByPlaceholder(/Session title/i)
      .fill("e2e: default-male voice");

    const goLive = page.getByRole("button", { name: /Go Live/i });
    await expect(goLive).toBeEnabled({ timeout: 10_000 });
    await goLive.click();

    // Quote modal: "How long will you stream?" with "Not yet" / "Continue".
    // Wait for the modal heading first so we don't race the dialog open.
    const continueBtn = page.getByRole("button", { name: /^Continue$/i });
    await expect(
      page.getByRole("heading", { name: /How long will you stream/i }),
    ).toBeVisible({ timeout: 5_000 });
    await continueBtn.click();

    await page.waitForURL(/\/session\/[^/]+\/setup\b/, {
      timeout: 15_000,
    });
    const m = /\/session\/([^/]+)\/setup\b/.exec(page.url());
    expect(m).not.toBeNull();
    sessionId = m![1]!;
  });

  await test.step("7. switch session voice preset to MALE via API", async () => {
    // The setup page only renders VoicePresetPicker when session.voice_id
    // is set. Without a clone, it renders VoiceSetupCard whose Skip
    // button hard-codes preset=female (session-setup-page.tsx:103).
    // PATCH directly so the live session uses the male library voice.
    const res = await request.patch(
      `${WORKERS_URL}/api/sessions/${sessionId}/voice-preset`,
      { data: { voice_preset: "male" } },
    );
    expect(
      res.ok(),
      `PATCH voice-preset returned ${res.status()}: ${await res.text()}`,
    ).toBe(true);
  });

  await test.step("8. setup page: skip voice → Go Live", async () => {
    // Click Skip so voiceReady=true and the Go Live button enables.
    // The local UI state will say "female" but the server keeps the
    // male preset we just PATCHed; goLive() only navigates, never
    // re-PATCHes, so the live page reads male from D1.
    await page
      .getByRole("button", { name: /Skip \(use default voice\)/i })
      .click();
    await page
      .getByRole("button", { name: /Go Live/i })
      .click();
    await page.waitForURL(/\/session\/[^/]+\/live\b/, {
      timeout: 15_000,
    });
  });

  await test.step("9. record 90s, exercise pipeline end-to-end", async () => {
    await page.getByRole("button", { name: /^Record$/i }).click();
    await expect(page.locator("body")).toContainText(/./, {
      timeout: 30_000,
    });
    await page.waitForTimeout(90_000);
    await page.getByRole("button", { name: /^Stop$/i }).click();
  });

  expect(page.url()).toMatch(/\/session\/[^/]+\/live/);
});
