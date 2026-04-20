// §0.2 — voice-clone cross-lingual invariant, full UI-to-wire chain.
//
// There are three separate tiers that each prove ONE slice of the
// invariant "clone enrolled in lang A, streaming target B → outgoing TTS
// request body carries language_code=A + model_id=eleven_flash_v2_5":
//
//   1. Unit (RTL): source-lang-picker + mismatch-banner rendering.
//   2. Integration (server-rs): server-rs/tests/tts_cross_lang_clone.rs
//      drives broadcast_translated_tts against a mock ElevenLabs and
//      pins the wire body. That's the tier where we assert the
//      language_code + model_id values end up in the outbound POST.
//   3. E2E (this file): proves the FRONTEND half of the chain —
//      onboarding selects source_lang, voice-clone POST carries it,
//      dashboard session creation carries source_lang + target_langs +
//      the voice_id returned by the upsert. A UI regression that silently
//      sends the wrong source_lang on clone (the exact drift class this
//      task flagged) fails this e2e at the POST intercept.
//
// The ElevenLabs wire intercept lives at tier #2, not here — Playwright
// doesn't drive server-rs in-process, so intercepting its outbound HTTP
// would require a full live stack (Soniox + ElevenLabs + real ffmpeg).
// Together #2 + this e2e cover the full chain: "what the FE sends" +
// "what server-rs does with that source_lang on the TTS call".
//
// Fails if, e.g.:
//   - onboarding ships source_lang=<browser-default> to POST /api/voices
//     instead of the user-selected "ko" (the UI-corruption class).
//   - dashboard overrides session.source_lang to something other than
//     the user-picked ko on POST /api/sessions (drift-on-create).
//   - active_voice_id plumbing loses the voice id the upsert returned.

import { test, expect, type APIRequestContext, type Page } from "@playwright/test";
import { MOCK } from "./config";

async function resetMock(req: APIRequestContext) {
  await req.post(`${MOCK}/test/reset`);
}

// Stub getUserMedia + AudioContext so useVoiceRecorder spins up without
// real hardware. Mirrors onboarding-new-user.e2e.ts — kept in-file so a
// refactor there can't silently break this test's assumptions.
async function stubMediaApis(page: Page) {
  await page.addInitScript(() => {
    class FakeProcessor {
      onaudioprocess:
        | ((e: { inputBuffer: { getChannelData: () => Float32Array } }) => void)
        | null = null;
      connect() {}
      disconnect() {}
    }
    class FakeSource {
      connect() {}
      disconnect() {}
    }
    class FakeAudioContext {
      destination = {};
      createMediaStreamSource() {
        return new FakeSource() as unknown as MediaStreamAudioSourceNode;
      }
      createScriptProcessor() {
        const p = new FakeProcessor();
        // One frame of silence — the mock accepts any audio body. The
        // fake clock below will drive the recorder's elapsedSec past
        // VOICE_MIN_SEC=30 without real wall-clock delay.
        setTimeout(
          () =>
            p.onaudioprocess?.({
              inputBuffer: { getChannelData: () => new Float32Array(4096) },
            }),
          50,
        );
        return p as unknown as ScriptProcessorNode;
      }
      close() {
        return Promise.resolve();
      }
    }
    Object.defineProperty(globalThis, "AudioContext", {
      configurable: true,
      value: FakeAudioContext,
    });
    if (!navigator.mediaDevices) {
      Object.defineProperty(navigator, "mediaDevices", {
        configurable: true,
        value: {
          getUserMedia: async () =>
            ({ getTracks: () => [{ stop: () => {} }] } as unknown as MediaStream),
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

test.describe("Scenario 20 — voice-clone cross-lingual full UI chain", () => {
  test.beforeEach(async ({ request }) => {
    await resetMock(request);
  });

  test("Korean clone → Japanese default → English target session: source_lang=ko flows end-to-end through /api/voices + /api/sessions", async ({
    page,
    request,
  }) => {
    // Page.clock intercepts Date.now + setInterval so we can fast-forward
    // past the recorder's 30-second minimum without blocking the test for
    // 30 real seconds. Install BEFORE page.goto so the in-page timers
    // mount against the fake clock from the very first render.
    await page.clock.install({ time: new Date("2026-04-20T12:00:00Z") });
    await stubMediaApis(page);

    // Pre-seed the user as YouTube-connected so the onboarding platform
    // step shows "connected" and we can "Continue" past it without
    // driving the real OAuth bounce (which redirects to /dashboard and
    // skips onboarding entirely).
    await request.post(`${MOCK}/test/seed-user`, {
      data: {
        user_id: "e2e-user",
        youtube_connected: true,
        youtube_channel_name: "Aziz",
      },
    });

    // Sign-in via the mocked Google flow. The home-page handler forwards
    // to /dashboard once user_id + JWT land in the URL, which then
    // bounces to /onboarding because onboarding_completed_at is null.
    await page.goto("/");
    await page.getByRole("button", { name: /Stream Dashboard/i }).click();
    await expect(
      page.getByRole("button", { name: /Sign in with Google/i }),
    ).toBeVisible();
    await page.getByRole("button", { name: /Sign in with Google/i }).click();
    await page.waitForURL(/\/onboarding/);

    // ── Step 1: platform. YouTube is pre-seeded connected → Continue.
    await expect(
      page.getByRole("heading", { name: /Connect a streaming platform/i }),
    ).toBeVisible();
    await page.getByRole("button", { name: /Continue/i }).click();

    // ── Step 2: voice clone. Pick Korean source_lang, start recording,
    // fast-forward 35 seconds of fake time, click Stop & Clone, and
    // intercept the outgoing POST /api/voices to assert source_lang.
    await expect(
      page.getByRole("heading", { name: /Clone your voice/i }),
    ).toBeVisible();

    // source-lang-picker exposes one radio per SOURCE_LANGS entry. Pick
    // Korean explicitly — this is the invariant that the UI must carry
    // through to the upsert payload.
    await page.getByRole("radio", { name: /Korean/i }).click();

    // Start recorder, wait for the recording indicator so we know the
    // AudioContext + setInterval tick registered, then fast-forward past
    // VOICE_MIN_SEC=30s so minReached flips and the stop button changes
    // its label from "30s to minimum" → "Stop & Clone".
    await page.getByRole("button", { name: /Record Voice Sample/i }).click();
    await expect(page.getByText(/Recording…/)).toBeVisible();
    await page.clock.fastForward(35_000);
    const stopButton = page.getByRole("button", { name: /Stop & Clone/i });
    await expect(stopButton).toBeEnabled();

    const voicePostPromise = page.waitForRequest(
      (req) =>
        req.method() === "POST" &&
        req.url().endsWith("/api/voices") &&
        !req.url().includes("test"),
    );
    await stopButton.click();
    const voicePost = await voicePostPromise;

    const voiceBody = voicePost.postDataJSON() as {
      user_id: string;
      source_lang: string;
      audio_base64: string;
      name: string;
    };
    // Core §0.2 invariant: the clone upsert must carry the enrollment
    // language the user picked in the radio — NOT the browser-default.
    // Any UI regression that silently resets source_lang on submit lands
    // this assertion in the failure diff.
    expect(voiceBody.source_lang).toBe("ko");
    expect(voiceBody.user_id).toBe("e2e-user");
    expect(voiceBody.audio_base64.length).toBeGreaterThan(0);

    // The mock's POST /api/voices response is the authoritative voice
    // row the dashboard will later wire into session.voice_id. Capture
    // it so the session assertion below can prove the pointer lines up.
    const voiceResp = await voicePost.response();
    const voiceResponseBody = (await voiceResp?.json()) as {
      id: string;
      source_lang: string;
      user_id: string;
    };
    expect(voiceResponseBody.source_lang).toBe("ko");

    // ── Step 3: default audience language = Japanese → Finish.
    // The "Continue" button at the bottom of step 2 is enabled once
    // voice is set. Click it after the upload resolves.
    await expect(
      page.getByText(/Voice cloned — Aziz/i),
    ).toBeVisible();
    await page.getByRole("button", { name: /^Continue$/i }).click();
    await expect(
      page.getByRole("heading", { name: /default audience language/i }),
    ).toBeVisible();
    await page.getByRole("button", { name: /Japanese/i }).click();
    await page.getByRole("button", { name: /Finish/i }).click();
    await page.waitForURL(/\/dashboard/);

    // Dashboard hydrates the user + voice rows — confirm active_voice_id
    // mirrors the upsert. A UI regression that leaves active_voice_id
    // null after clone lands here, not in a stale downstream flow.
    const userAfter = await request.get(`${MOCK}/api/user?user_id=e2e-user`);
    const userJson = (await userAfter.json()) as { active_voice_id: string | null };
    expect(userJson.active_voice_id).toBe(voiceResponseBody.id);

    // ── Create a session with target_langs=[en]. The voice is enrolled
    // in ko, the default audience lang was ja, but THIS session targets
    // en. The server-rs TTS dispatcher will pin language_code=ko on the
    // outbound ElevenLabs POST (covered by
    // server-rs/tests/tts_cross_lang_clone.rs). Here we prove the
    // frontend hands Workers a session with source_lang=ko,
    // target_langs=[en], voice_id=<the just-cloned voice id>.
    await page.getByRole("button", { name: /Add destination/i }).click();
    await page.getByRole("button", { name: /Local Test/i }).click();

    // The destination's lang picker is the last combobox on the page.
    const combos = page.getByRole("combobox");
    await combos.last().selectOption("en");

    await page.getByPlaceholder("Session title").fill("E2E full-chain ko→en");

    const sessionPostPromise = page.waitForRequest(
      (req) =>
        req.method() === "POST" &&
        /\/api\/sessions$/.test(req.url().split("?")[0]!),
    );
    await page.getByRole("button", { name: /Go Live/i }).click();
    const sessionPost = await sessionPostPromise;

    const sessionBody = sessionPost.postDataJSON() as {
      user_id: string;
      source_lang: string;
      target_langs: string[];
      voice_id?: string;
      platforms: Array<{ lang: string }>;
    };
    // Full-chain invariant: the voice enrolled in ko + the session
    // source anchored in ko + a DIFFERENT target lang (en) is the exact
    // shape server-rs/tts_cross_lang_clone.rs asserts downstream. If any
    // part of that chain drifts in the UI, the wire payload captured
    // here stops matching.
    expect(sessionBody.source_lang).toBe("ko");
    expect(sessionBody.target_langs).toEqual(["en"]);
    expect(sessionBody.voice_id).toBe(voiceResponseBody.id);
    expect(sessionBody.platforms.some((p) => p.lang === "en")).toBe(true);

    // The quote modal pops after create-session — reaching it is the
    // canonical "session POST resolved cleanly" signal. Don't traverse
    // the rest of the go-live flow; other e2e files cover it.
    await expect(
      page.getByRole("heading", { name: /How long will you stream/i }),
    ).toBeVisible();
  });
});
