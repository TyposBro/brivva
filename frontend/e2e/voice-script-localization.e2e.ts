import { test, expect, type APIRequestContext, type Page } from "@playwright/test";
import { MOCK } from "./config";

// Stable substrings pulled from the script bodies in voice-setup-card.tsx's
// SAMPLE_SCRIPTS map. Picked long enough (≥4 chars of script-specific glyphs)
// that a single-character false positive is impossible — the English
// pangram is unambiguous, and each CJK script has its own codepoint block.
const LANG_CASES = [
  { code: "ko", radio: /Korean/i, substring: "여러분 안녕하세요" },
  { code: "en", radio: /English/i, substring: "quick brown fox" },
  { code: "ja", radio: /Japanese/i, substring: "みなさん" },
  { code: "zh", radio: /Chinese/i, substring: "大家晚上好" },
] as const;

async function resetMock(req: APIRequestContext) {
  await req.post(`${MOCK}/test/reset`);
}

// Same AudioContext + getUserMedia shim as the existing onboarding e2e.
// Voice step's "Record Voice Sample" button triggers recorder.start(),
// which needs getUserMedia + AudioContext to resolve without real hardware.
async function stubMediaApis(page: Page) {
  await page.addInitScript(() => {
    class FakeProcessor {
      onaudioprocess: ((e: { inputBuffer: { getChannelData: () => Float32Array } }) => void) | null = null;
      connect() {}
      disconnect() {}
    }
    class FakeSource { connect() {} disconnect() {} }
    class FakeAudioContext {
      destination = {};
      createMediaStreamSource() { return new FakeSource() as unknown as MediaStreamAudioSourceNode; }
      createScriptProcessor() {
        const p = new FakeProcessor();
        setTimeout(() => p.onaudioprocess?.({
          inputBuffer: { getChannelData: () => new Float32Array(4096) },
        }), 50);
        return p as unknown as ScriptProcessorNode;
      }
      close() { return Promise.resolve(); }
    }
    Object.defineProperty(globalThis, "AudioContext", { configurable: true, value: FakeAudioContext });
    if (!navigator.mediaDevices) {
      Object.defineProperty(navigator, "mediaDevices", {
        configurable: true,
        value: {
          getUserMedia: async () => ({ getTracks: () => [{ stop: () => {} }] } as unknown as MediaStream),
        },
      });
    }
  });
}

async function landOnVoiceStep(page: Page) {
  // Fresh user → Google SSO mock round-trip → /onboarding step 1 → skip to
  // voice step. Mirrors onboarding-new-user.e2e.ts's flow.
  await page.goto("/");
  await page.getByRole("button", { name: /Stream Dashboard/i }).click();
  await page.getByRole("button", { name: /Sign in with Google/i }).click();
  await page.waitForURL(/\/onboarding/);
  await expect(
    page.getByRole("heading", { name: /Connect a streaming platform/i }),
  ).toBeVisible();
  await page.getByRole("button", { name: /Skip for now/i }).click();
  await expect(
    page.getByRole("heading", { name: /Clone your voice/i }),
  ).toBeVisible();
}

test.describe("Scenario 5 — localized voice-clone sample script", () => {
  test.beforeEach(async ({ request }) => {
    await resetMock(request);
  });

  for (const { code, radio, substring } of LANG_CASES) {
    test(`source-lang=${code} → script renders native text + lang attribute`, async ({ page }) => {
      await stubMediaApis(page);
      await landOnVoiceStep(page);

      // Pick the language via the radio group. The picker uses role="radio"
      // buttons labeled with the language name; clicking re-seeds the
      // `sourceLang` state that the VoiceSetupCard reads.
      await page.getByRole("radio", { name: radio }).click();

      // Start recording. The recorder enters `isRecording=true` → the card
      // flips from the "Record Voice Sample" CTA to the live-recording view,
      // which is the only state that renders the sample script.
      await page.getByRole("button", { name: /Record Voice Sample/i }).click();

      // The script wrapper is a <div lang={sourceLang}> in voice-setup-card.tsx.
      // Match on the language-specific substring and assert the lang attr.
      const scriptBlock = page.locator(`div[lang="${code}"]`);
      await expect(scriptBlock).toBeVisible();
      await expect(scriptBlock).toContainText(substring);

      // Other languages' hints must NOT leak into this render — regression
      // guard against SAMPLE_SCRIPTS being keyed wrong.
      const otherSubstrings = LANG_CASES
        .filter((c) => c.code !== code)
        .map((c) => c.substring);
      const blockText = (await scriptBlock.textContent()) ?? "";
      for (const other of otherSubstrings) {
        expect(blockText).not.toContain(other);
      }
    });
  }
});
