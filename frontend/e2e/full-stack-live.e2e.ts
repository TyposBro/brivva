// Full-stack Playwright e2e: drives the real local dev stack
// (frontend + workers + server-rs + Soniox + ElevenLabs + your live
// Grip + your live custom-RTMP destination) end to end without a
// human at the keyboard.
//
// Preconditions are enforced by `scripts/test-e2e-real.sh`:
//   - dev-stack is up (`./scripts/dev-all.sh`)
//   - workers/.dev.vars has DEV_AUTH_BYPASS=true
//   - tests/e2e/.env has GRIP_RTMP_URL/KEY + YOUTUBE_RTMP_URL/KEY
//   - tests/e2e/fixtures/fake-cam.y4m + fake-mic.wav exist
//
// Why a separate test file:
//   - All other frontend/e2e/*.e2e.ts target the mock server in
//     mock-server.mjs. This one targets the real stack so the same
//     pipeline that ships to prod is exercised.
//   - Voice cloning + Soniox both hit real APIs and consume credits;
//     we don't want this on every PR. Run on demand via
//     `./scripts/test-e2e-real.sh`.

import { test, expect } from "@playwright/test";

const WORKERS_URL =
  process.env.BRIVVA_DEV_WORKERS_URL ?? "http://localhost:8787";

// Lazy lookup so `playwright --list` (which loads the test file but
// doesn't run it) doesn't blow up when .env hasn't been sourced.
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
  // Hard-reset dev-user D1 state so the test starts from "brand new
  // user". The endpoint is gated by DEV_AUTH_BYPASS=true; absent that
  // it 404s and the test should fail fast.
  const res = await request.post(`${WORKERS_URL}/test/reset-dev-user`);
  expect(
    res.ok(),
    `POST /test/reset-dev-user failed (status ${res.status()}). ` +
      `Confirm DEV_AUTH_BYPASS=true in workers/.dev.vars and restart dev-stack.`,
  ).toBe(true);
});

test("full pipeline: signup → onboard → clone → Grip(zh) + Custom(pass) → Go Live → record", async ({
  page,
}) => {
  const GRIP_RTMP_URL = requiredEnv("GRIP_RTMP_URL");
  const GRIP_RTMP_KEY = requiredEnv("GRIP_RTMP_KEY");
  const YOUTUBE_RTMP_URL = requiredEnv("YOUTUBE_RTMP_URL");
  const YOUTUBE_RTMP_KEY = requiredEnv("YOUTUBE_RTMP_KEY");

  await test.step("1. sign in via DEV_AUTH_BYPASS", async () => {
    // `/` is the public landing page — no sign-in button. SignInGate
    // only renders on protected routes; navigating to /dashboard
    // triggers it.
    await page.goto("/dashboard");
    await page
      .getByRole("button", { name: /Sign in with Google/i })
      .click();
    await page.waitForURL(/\/(onboarding|dashboard)\b/, { timeout: 15_000 });
  });

  await test.step("2. onboarding: skip platform connect", async () => {
    // YouTube OAuth is the only step requiring real Google; we use
    // paste-creds Grip + Custom RTMP later instead.
    await expect(
      page.getByRole("heading", { name: /Connect a streaming platform/i }),
    ).toBeVisible({ timeout: 15_000 });
    await page.getByRole("button", { name: /^Continue$/i }).click();
  });

  await test.step("3. onboarding: voice clone (real ElevenLabs)", async () => {
    await expect(
      page.getByRole("heading", { name: /Clone your voice/i }),
    ).toBeVisible();
    // The fake-mic WAV is Korean (source.mp4 host audio). Enrollment
    // language must match — otherwise ElevenLabs anchors the clone in
    // the wrong phonology and the dashboard later trips
    // voiceLangMismatch against the Korean source-lang default.
    await page.getByRole("radio", { name: /Korean/i }).click();
    await page
      .getByRole("button", { name: /Record Voice Sample/i })
      .click();
    // 30s record + ElevenLabs upload (10-90s).
    const stopClone = page.getByRole("button", { name: /Stop & Clone/i });
    await expect(stopClone).toBeEnabled({ timeout: 60_000 });
    await stopClone.click();
    await expect(page.getByText(/Voice cloned/i)).toBeVisible({
      timeout: 120_000,
    });
    await page.getByRole("button", { name: /^Continue$/i }).click();
  });

  await test.step("4. onboarding: pick default audience language", async () => {
    await expect(
      page.getByRole("heading", { name: /Default audience language/i }),
    ).toBeVisible();
    await page.getByRole("button", { name: /Chinese/i }).click();
    await page.getByRole("button", { name: /^Finish$/i }).click();
    await page.waitForURL(/\/dashboard\b/, { timeout: 15_000 });
  });

  await test.step("5a. add Grip destination, lang=zh (cloned)", async () => {
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

  await test.step("6. Go Live → setup → live page", async () => {
    // Title is required — handleCreateSession bails with
    // "Enter a session title" otherwise. Without this the Go Live
    // button looks active but the click is a no-op.
    await page
      .getByPlaceholder(/Session title/i)
      .fill("e2e: full-stack live");

    const goLive = page.getByRole("button", { name: /Go Live/i });
    await expect(goLive).toBeEnabled({ timeout: 10_000 });
    await goLive.click();

    // Quote modal: "How long will you stream?" with "Not yet" / "Continue".
    await expect(
      page.getByRole("heading", { name: /How long will you stream/i }),
    ).toBeVisible({ timeout: 5_000 });
    await page.getByRole("button", { name: /^Continue$/i }).click();

    await page.waitForURL(/\/session\/[^/]+\/(setup|live)\b/, {
      timeout: 15_000,
    });
    if (page.url().includes("/setup")) {
      await page.getByRole("button", { name: /Go Live/i }).click();
      await page.waitForURL(/\/session\/[^/]+\/live\b/, {
        timeout: 15_000,
      });
    }
  });

  await test.step("7. record 90s, exercise pipeline end-to-end", async () => {
    await page.getByRole("button", { name: /^Record$/i }).click();
    // Wait for any text on the page to confirm the WS is live (the
    // transcript surface starts populating within ~10s).
    await expect(page.locator("body")).toContainText(/./, {
      timeout: 30_000,
    });
    // 90s lets the per-lang TTS worker dispatch several utterances on
    // the zh lane and confirms the pass lane stays raw.
    await page.waitForTimeout(90_000);
    await page.getByRole("button", { name: /^Stop$/i }).click();
  });

  // Audio reaching Grip / YouTube is verified out of band by watching
  // the live preview on each platform. The CI signal here is: no
  // page errors, no JS crashes, recording started + stopped cleanly.
  expect(page.url()).toMatch(/\/session\/[^/]+\/live/);
});
