// Parameterized full-stack e2e against the REAL local dev stack
// (frontend + workers + server-rs + Soniox + ElevenLabs + your live
// Grip + custom-RTMP destinations). NOT the mock server.
//
// Driven entirely by env vars set by `scripts/test-e2e-real.sh`. The
// shell script parses CLI flags (`--source ko --grip en --youtube zh
// --rtmp ja --rtmp2 ko`), sources `tests/e2e/.env`, then exports the
// resulting `E2E_*` config + per-platform RTMP credentials before
// invoking Playwright. This file is a thin orchestrator.
//
// See `scripts/test-e2e-real.sh --help` for the flag matrix and
// `tests/e2e/README.md` for a recommended combination table.

import { test, expect, type Page, type Locator } from "@playwright/test";

const WORKERS_URL =
  process.env.BRIVVA_DEV_WORKERS_URL ?? "http://localhost:8787";

type Lang = "en" | "ko" | "ja" | "zh";
type DestPlatform = "grip" | "custom";

interface DestConfig {
  /** UI tile to click in the platform picker. */
  platform: DestPlatform;
  /** Which env-var pair the credentials live under. */
  credPrefix: "GRIP_RTMP" | "YOUTUBE_RTMP" | "RTMP" | "RTMP2";
  /** Target language. If equal to the session source-lang, the test
   *  selects "pass" (raw passthrough) instead of issuing translation. */
  lang: Lang;
  /** Pretty label for log lines. */
  label: string;
}

function envLang(name: string): Lang | null {
  const v = (process.env[name] ?? "").trim();
  if (!v) return null;
  if (v !== "en" && v !== "ko" && v !== "ja" && v !== "zh") {
    throw new Error(`${name} must be one of en|ko|ja|zh, got '${v}'`);
  }
  return v;
}

function requiredEnv(name: string): string {
  const v = process.env[name];
  if (!v || v.startsWith("YOUR-") || v.includes("YOUR_")) {
    throw new Error(
      `${name} missing or still a placeholder. Edit tests/e2e/.env.`,
    );
  }
  return v;
}

function buildPlan(): {
  clone: boolean;
  sourceLang: Lang;
  recordSeconds: number;
  destinations: DestConfig[];
} {
  const sourceLang = (envLang("E2E_SOURCE_LANG") ?? "ko") as Lang;
  const clone = (process.env.E2E_CLONE ?? "true") !== "false";
  const recordSeconds = Number(process.env.E2E_RECORD_SECONDS ?? "90");
  const destinations: DestConfig[] = [];
  const grip = envLang("E2E_GRIP_LANG");
  if (grip)
    destinations.push({
      platform: "grip",
      credPrefix: "GRIP_RTMP",
      lang: grip,
      label: "grip",
    });
  const youtube = envLang("E2E_YOUTUBE_LANG");
  if (youtube)
    destinations.push({
      platform: "custom",
      credPrefix: "YOUTUBE_RTMP",
      lang: youtube,
      label: "youtube",
    });
  const rtmp = envLang("E2E_RTMP_LANG");
  if (rtmp)
    destinations.push({
      platform: "custom",
      credPrefix: "RTMP",
      lang: rtmp,
      label: "rtmp",
    });
  const rtmp2 = envLang("E2E_RTMP2_LANG");
  if (rtmp2)
    destinations.push({
      platform: "custom",
      credPrefix: "RTMP2",
      lang: rtmp2,
      label: "rtmp2",
    });
  if (destinations.length === 0) {
    throw new Error(
      "No destinations configured. Pass at least one of " +
        "--grip/--youtube/--rtmp/--rtmp2 to test-e2e-real.sh.",
    );
  }
  return { clone, sourceLang, recordSeconds, destinations };
}

const LANG_LABEL: Record<Lang, RegExp> = {
  en: /English/i,
  ko: /Korean/i,
  ja: /Japanese/i,
  zh: /Chinese/i,
};

/** Helper: sentinel "pass" gets selected when dest.lang == sourceLang. */
function effectiveDestLang(dest: Lang, sourceLang: Lang): string {
  return dest === sourceLang ? "pass" : dest;
}

test.beforeEach(async ({ request }) => {
  const res = await request.post(`${WORKERS_URL}/test/reset-dev-user`);
  expect(
    res.ok(),
    `POST /test/reset-dev-user failed (status ${res.status()}). ` +
      `Confirm DEV_AUTH_BYPASS=true in workers/.dev.vars and restart dev-stack.`,
  ).toBe(true);
});

test("real-stack pipeline (params from E2E_* env)", async ({
  page,
  request,
}) => {
  const plan = buildPlan();
  const planSummary = plan.destinations
    .map(
      (d) =>
        `${d.label}=${d.lang === plan.sourceLang ? `${d.lang}(pass)` : d.lang}`,
    )
    .join(", ");
  test.info().annotations.push({
    type: "plan",
    description: `clone=${plan.clone} source=${plan.sourceLang} dests=[${planSummary}] record=${plan.recordSeconds}s`,
  });

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

  if (plan.clone) {
    await test.step(
      `3. onboarding: clone voice in ${plan.sourceLang} (real ElevenLabs)`,
      async () => {
        await expect(
          page.getByRole("heading", { name: /Clone your voice/i }),
        ).toBeVisible();
        // Enrollment language must match the WAV; mismatch trips
        // voiceLangMismatch on the dashboard.
        await page
          .getByRole("radio", { name: LANG_LABEL[plan.sourceLang] })
          .click();
        await page
          .getByRole("button", { name: /Record Voice Sample/i })
          .click();
        const stopClone = page.getByRole("button", {
          name: /Stop & Clone/i,
        });
        await expect(stopClone).toBeEnabled({ timeout: 60_000 });
        await stopClone.click();
        await expect(page.getByText(/Voice cloned/i)).toBeVisible({
          timeout: 120_000,
        });
        await page.getByRole("button", { name: /^Continue$/i }).click();
      },
    );
  } else {
    await test.step("3. onboarding: skip voice clone", async () => {
      await expect(
        page.getByRole("heading", { name: /Clone your voice/i }),
      ).toBeVisible();
      await page
        .getByRole("button", { name: /Skip \(use default voice\)/i })
        .click();
    });
  }

  await test.step("4. onboarding: pick default audience language", async () => {
    await expect(
      page.getByRole("heading", { name: /Default audience language/i }),
    ).toBeVisible();
    // Pick any non-source lang (the picker filters out the source).
    // The dashboard overrides this per-destination anyway.
    const fallback: Lang = plan.sourceLang === "ko" ? "en" : "ko";
    await page
      .getByRole("button", { name: LANG_LABEL[fallback] })
      .click();
    await page.getByRole("button", { name: /^Finish$/i }).click();
    await page.waitForURL(/\/dashboard\b/, { timeout: 15_000 });
  });

  await test.step(
    `5. dashboard: set session source-lang = ${plan.sourceLang}`,
    async () => {
      await page
        .getByTestId("session-source-lang")
        .selectOption(plan.sourceLang);
    },
  );

  // Add destinations one by one. Custom RTMPs land in the SAME testid
  // bucket, so we filter by the CURRENT count to scope the new card.
  let customCount = 0;
  for (const [i, dest] of plan.destinations.entries()) {
    const url = requiredEnv(`${dest.credPrefix}_URL`);
    const key = requiredEnv(`${dest.credPrefix}_KEY`);
    const effective = effectiveDestLang(dest.lang, plan.sourceLang);
    await test.step(
      `6.${i + 1} add ${dest.label} dest, lang=${effective}${effective === "pass" ? " (auto: source==dest)" : ""}`,
      async () => {
        await page
          .getByRole("button", { name: /Add destination/i })
          .click();
        if (dest.platform === "grip") {
          await page.getByRole("button", { name: /^Grip$/i }).click();
        } else {
          await page
            .getByRole("button", { name: /Custom RTMP/i })
            .click();
        }
        const card: Locator =
          dest.platform === "grip"
            ? page.getByTestId("destination-card-grip")
            : page.getByTestId("destination-card-custom").nth(customCount);
        if (dest.platform === "custom") customCount += 1;
        await expect(card).toBeVisible();
        await card.getByPlaceholder(/Server URL/i).fill(url);
        await card.getByPlaceholder(/Stream Key/i).fill(key);
        await card.locator("select").first().selectOption(effective);
      },
    );
  }

  let sessionId = "";
  await test.step("7. Go Live → setup page", async () => {
    await page
      .getByPlaceholder(/Session title/i)
      .fill(`e2e: ${plan.clone ? "clone" : "default-male"} ${plan.sourceLang}`);
    const goLive = page.getByRole("button", { name: /Go Live/i });
    await expect(goLive).toBeEnabled({ timeout: 10_000 });
    await goLive.click();

    // Quote modal: "How long will you stream?" → Continue.
    await expect(
      page.getByRole("heading", { name: /How long will you stream/i }),
    ).toBeVisible({ timeout: 5_000 });
    await page.getByRole("button", { name: /^Continue$/i }).click();

    await page.waitForURL(/\/session\/[^/]+\/setup\b/, {
      timeout: 15_000,
    });
    const m = /\/session\/([^/]+)\/setup\b/.exec(page.url());
    expect(m, "expected /session/:id/setup URL").not.toBeNull();
    sessionId = m![1]!;
  });

  if (!plan.clone) {
    await test.step(
      "7b. switch session voice preset to MALE via API",
      async () => {
        // Setup page only renders VoicePresetPicker when session.voice_id
        // exists. Without a clone we get VoiceSetupCard whose Skip button
        // hard-codes preset=female. PATCH directly so the live session
        // uses the male library voice.
        const res = await request.patch(
          `${WORKERS_URL}/api/sessions/${sessionId}/voice-preset`,
          { data: { voice_preset: "male" } },
        );
        expect(
          res.ok(),
          `PATCH voice-preset returned ${res.status()}: ${await res.text()}`,
        ).toBe(true);
      },
    );
  }

  await test.step("8. setup page: skip voice → Go Live → /live", async () => {
    if (plan.clone) {
      // Cloned path: voiceReady is true once the picker mounts; just
      // click Go Live. Picker preset is left at whatever the bundle
      // ships with (cloned by default).
      await page.getByRole("button", { name: /Go Live/i }).click();
    } else {
      await page
        .getByRole("button", { name: /Skip \(use default voice\)/i })
        .click();
      await page.getByRole("button", { name: /Go Live/i }).click();
    }
    await page.waitForURL(/\/session\/[^/]+\/live\b/, {
      timeout: 15_000,
    });
  });

  await test.step(
    `9. record ${plan.recordSeconds}s, exercise pipeline end-to-end`,
    async () => {
      await page.getByRole("button", { name: /^Record$/i }).click();
      await expect(page.locator("body")).toContainText(/./, {
        timeout: 30_000,
      });
      await page.waitForTimeout(plan.recordSeconds * 1000);
      await page.getByRole("button", { name: /^Stop$/i }).click();
    },
  );

  // Out-of-band verification (live preview on each platform) is the
  // real signal. The CI assertion is just that the pipeline didn't
  // crash and the page survived to /live.
  expect(page.url()).toMatch(/\/session\/[^/]+\/live/);
});
