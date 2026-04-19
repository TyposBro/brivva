import { test, expect, type Page, type APIRequestContext } from "@playwright/test";

const MOCK = "http://localhost:8787";

async function resetMock(req: APIRequestContext) {
  await req.post(`${MOCK}/test/reset`);
}

// Stub Web Audio + MediaRecorder before any app code runs so the voice
// recorder hook can mount without poking at real hardware (Playwright's
// --use-fake-device flag covers getUserMedia, but the AudioContext +
// ScriptProcessorNode plumbing still needs deterministic surrogates).
async function stubMediaApis(page: Page) {
  await page.addInitScript(() => {
    class FakeProcessor {
      onaudioprocess: ((e: { inputBuffer: { getChannelData: () => Float32Array } }) => void) | null = null;
      connect() {}
      disconnect() {}
    }
    class FakeSource {
      connect() {}
      disconnect() {}
    }
    class FakeAudioContext {
      destination = {};
      createMediaStreamSource() { return new FakeSource() as unknown as MediaStreamAudioSourceNode; }
      createScriptProcessor() {
        const p = new FakeProcessor();
        // Push one frame of silence each tick so the encoded sample isn't empty.
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
    } else if (!navigator.mediaDevices.getUserMedia) {
      // @ts-expect-error — patch in jsdom-like environments
      navigator.mediaDevices.getUserMedia = async () => ({
        getTracks: () => [{ stop: () => {} }],
      });
    }
  });
}

test.describe("Scenario 1 — new user full onboarding", () => {
  test.beforeEach(async ({ request }) => {
    await resetMock(request);
  });

  test("Sign in with Google → /onboarding → 3 steps → /dashboard", async ({ page }) => {
    await stubMediaApis(page);
    await page.goto("/");

    await page.getByRole("button", { name: /Stream Dashboard/i }).click();
    // Dashboard sees no signed-in user → SignInGate prompt.
    await expect(page.getByRole("button", { name: /Sign in with Google/i })).toBeVisible();
    await page.getByRole("button", { name: /Sign in with Google/i }).click();

    // Mock /auth/google bounces back to / with user_id + JWT in the fragment;
    // the home-page handler then forwards to /dashboard, which (on first
    // visit) bounces to /onboarding because onboarding_completed_at is null.
    await page.waitForURL(/\/onboarding/);

    // ── Step 1 — connect a platform. Skip for the e2e (no real OAuth).
    await expect(
      page.getByRole("heading", { name: /Connect a streaming platform/i }),
    ).toBeVisible();
    await page.getByRole("button", { name: /Skip for now/i }).click();

    // ── Step 2 — voice. The mocked audio context will let the recorder
    // start and the upload hits /api/voices on the mock server.
    await expect(
      page.getByRole("heading", { name: /Clone your voice/i }),
    ).toBeVisible();
    await page.getByRole("button", { name: /Skip for now/i }).click();

    // ── Step 3 — pick default audience language and finish.
    await expect(
      page.getByRole("heading", { name: /default audience language/i }),
    ).toBeVisible();
    await page.getByRole("button", { name: /Japanese/i }).click();
    await page.getByRole("button", { name: /Finish/i }).click();

    // Onboarding completed → dashboard.
    await page.waitForURL(/\/dashboard/);

    // Verify Workers saw the completion call.
    const stateAfter = await page.request.get(`${MOCK}/api/user?user_id=e2e-user`);
    const body = await stateAfter.json();
    expect(body.onboarding_completed_at).not.toBeNull();
  });
});
