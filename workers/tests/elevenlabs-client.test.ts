import { describe, it, expect, vi, afterEach } from "vitest";

import {
  cloneVoice,
  deleteRemoteVoice,
} from "../src/features/voices/elevenlabs-client";

const KEY = "test-el-key";
const AUDIO_B64 = btoa("\0\0\0\0\0\0\0\0");

afterEach(() => vi.unstubAllGlobals());

// Box the captured FormData so TypeScript's flow analysis doesn't narrow the
// outer variable to `null` based on the initial assignment.
type Capture = { form: FormData | null };

describe("elevenlabs cloneVoice", () => {
  it("sends multipart with name + language label (happy)", async () => {
    const cap: Capture = { form: null };
    vi.stubGlobal(
      "fetch",
      vi.fn(async (_u: RequestInfo | URL, init?: RequestInit) => {
        const body = init?.body;
        cap.form = body instanceof FormData ? body : null;
        return new Response(JSON.stringify({ voice_id: "el-1" }), { status: 200 });
      }),
    );
    const r = await cloneVoice(KEY, {
      name: "Host",
      audioBase64: AUDIO_B64,
      sourceLang: "ko",
    });
    expect(r.voice_id).toBe("el-1");
    expect(cap.form?.get("name")).toBe("Host");
    expect(cap.form?.get("labels")).toBe(JSON.stringify({ language: "ko" }));
  });

  it("skips labels field when sourceLang is null (edge)", async () => {
    const cap: Capture = { form: null };
    vi.stubGlobal(
      "fetch",
      vi.fn(async (_u: RequestInfo | URL, init?: RequestInit) => {
        const body = init?.body;
        cap.form = body instanceof FormData ? body : null;
        return new Response(JSON.stringify({ voice_id: "el-2" }), { status: 200 });
      }),
    );
    await cloneVoice(KEY, {
      name: "Host2",
      audioBase64: AUDIO_B64,
      sourceLang: null,
    });
    expect(cap.form?.get("labels")).toBeNull();
  });

  it("throws with upstream status + body on non-ok (sad)", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => new Response("quota exceeded", { status: 402 })),
    );
    await expect(
      cloneVoice(KEY, { name: "n", audioBase64: AUDIO_B64, sourceLang: null }),
    ).rejects.toThrow(/ElevenLabs clone failed: 402/);
  });
});

describe("elevenlabs deleteRemoteVoice", () => {
  it("URL-encodes the voice id in the path (happy)", async () => {
    const cap: { url: string | null } = { url: null };
    vi.stubGlobal(
      "fetch",
      vi.fn(async (u: RequestInfo | URL) => {
        cap.url = typeof u === "string" ? u : u instanceof URL ? u.toString() : u.url;
        return new Response("", { status: 200 });
      }),
    );
    await deleteRemoteVoice(KEY, "voice/with/slashes");
    expect(cap.url).toContain("voice%2Fwith%2Fslashes");
  });
});
