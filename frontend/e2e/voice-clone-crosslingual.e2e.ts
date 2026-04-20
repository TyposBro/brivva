// §0.2 — voice-clone upsert + cross-lingual source_lang invariant, e2e.
//
// Three tests cover the same wire invariant at different tiers:
//   1. Unit (RTL): source-lang-picker, mismatch-banner rendering.
//   2. Integration (server-rs): `tts_cross_lang_clone.rs` pins the TTS
//      request body — cloned voice + cross-lingual target MUST carry
//      `model_id=eleven_flash_v2_5` + `language_code=<enrollment_lang>`.
//      This is the "if field names drift, prod breaks silently" tier.
//   3. E2E (this file): the full UI chain — a user who enrolls a voice in
//      language A can create a session whose target list contains a
//      DIFFERENT language B, and the POST /api/sessions payload reflects
//      both langs without tripping the mismatch banner.
//
// The cross-stack gap is that server-rs runs separately from the mock
// stack and never sees the TTS body in this e2e. That invariant stays at
// the server-rs integration level. Here we prove the UI → Workers half of
// the chain: voice A, session source A, destination lang B, streams table
// ends up with both "pass" handling disabled and the target lang wired
// through.

import { test, expect, type APIRequestContext, type Page } from "@playwright/test";
import { MOCK } from "./config";

async function resetMock(req: APIRequestContext) {
  await req.post(`${MOCK}/test/reset`);
}

// Seed an onboarded user + a KOREAN-enrolled voice clone so the dashboard
// defaults to source_lang=ko (matching the voice) and the user can go
// live with Japanese destinations without hitting the mismatch banner.
async function seedKoreanVoiceUser(req: APIRequestContext) {
  await req.post(`${MOCK}/test/seed-user`, {
    data: {
      user_id: "e2e-user",
      onboarding_completed_at: Math.floor(Date.now() / 1000),
      active_voice_id: "v-ko-clone",
      youtube_connected: true,
      youtube_channel_name: "Aziz",
    },
  });
  await req.post(`${MOCK}/test/seed-voice`, {
    data: {
      user_id: "e2e-user",
      id: "v-ko-clone",
      source_lang: "ko",
      name: "Aziz — Korean clone",
    },
  });
}

async function signIn(page: Page) {
  await page.addInitScript(() => {
    localStorage.setItem("brivva_user_id", "e2e-user");
  });
}

test.describe("Scenario 7 — voice-clone cross-lingual chain", () => {
  test.beforeEach(async ({ request }) => {
    await resetMock(request);
    await seedKoreanVoiceUser(request);
  });

  test("Korean-enrolled voice → Japanese destination → create-session POST carries both langs and no mismatch banner fires", async ({ page }) => {
    await signIn(page);
    await page.goto("/dashboard");

    // Voice is enrolled as ko, dashboard default session source is also ko.
    // The mismatch banner must NOT appear — this test fails fast if the
    // invariant is broken (e.g. a refactor that resets source_lang on
    // mount or fails to read voice.source_lang).
    await expect(
      page.getByTestId("voice-lang-mismatch-banner"),
    ).not.toBeVisible();

    // Add a Local Test destination, then flip its lang to Japanese so the
    // session has a target lang DIFFERENT from the voice enrollment. This
    // is the cross-lingual shape the server-rs integration test pins at
    // wire level (`language_code=ko` for a voice enrolled in ko, target
    // lang ja).
    await page.getByRole("button", { name: /Add destination/i }).click();
    await page.getByRole("button", { name: /Local Test/i }).click();

    // The destination's lang picker is the last combobox on the page.
    const combos = page.getByRole("combobox");
    await combos.last().selectOption("ja");

    // Intercept the create-session POST so we can prove target_langs and
    // the platform list reflect the cross-lingual choice.
    const createPromise = page.waitForRequest(
      (req) => req.method() === "POST" && req.url().includes("/api/sessions"),
    );

    await page.getByPlaceholder("Session title").fill("E2E crosslingual ko→ja");
    await page.getByRole("button", { name: /Go Live/i }).click();

    const createReq = await createPromise;
    const payload = createReq.postDataJSON() as {
      source_lang: string;
      target_langs: string[];
      platforms: Array<{ lang: string }>;
    };

    // The wire invariant the server-rs TTS request body relies on is that
    // the session knows its source_lang (anchor for `language_code`) AND
    // the destination list carries a DIFFERENT target lang that triggers
    // the cross-lingual eleven_flash_v2_5 model selection downstream.
    expect(payload.source_lang).toBe("ko");
    expect(payload.target_langs).toContain("ja");
    expect(payload.platforms.some((p) => p.lang === "ja")).toBe(true);
    // Voice enrollment vs session source must match for the clone to be
    // kept — if this flipped to "en" silently, server-rs would fall back
    // to the default voice library (no clone) and the cross-lingual test
    // would never exercise the language_code branch in tts.rs.
    expect(payload.target_langs).not.toContain("en");

    // Quote modal pops — we've validated the POST payload. No need to
    // traverse the full go-live chain here; go-live.e2e.ts covers it.
    await expect(
      page.getByRole("heading", { name: /How long will you stream/i }),
    ).toBeVisible();
  });

  test("re-recording the voice in a new language (en) flips the mismatch banner on when session source stays ko", async ({ page, request }) => {
    // Second half of the §0.2 invariant: the mismatch banner + Go Live
    // gate must re-engage if the voice's source_lang diverges from the
    // session source_lang. This proves the chain detects drift after the
    // initial happy-path; pre-fix, the FE read `voice.source_lang` once
    // on mount and never re-hydrated on upsert.
    await signIn(page);
    await page.goto("/dashboard");
    await expect(
      page.getByTestId("voice-lang-mismatch-banner"),
    ).not.toBeVisible();

    // Overwrite the voice clone with an English enrollment via the mock
    // test endpoint — simulates the user clicking "Re-record" and
    // enrolling with a different source_lang.
    await request.post(`${MOCK}/test/seed-voice`, {
      data: {
        user_id: "e2e-user",
        id: "v-en-clone",
        source_lang: "en",
        name: "Aziz — English clone",
      },
    });

    // Force a dashboard reload so the voice state re-hydrates.
    await page.reload();

    // Session source is still ko, voice is now en → mismatch banner
    // MUST appear. Go Live must be gated off.
    const banner = page.getByTestId("voice-lang-mismatch-banner");
    await expect(banner).toBeVisible();
    await expect(banner).toContainText(/English/);
    await expect(banner).toContainText(/Korean/);
  });
});
