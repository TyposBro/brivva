import { describe, it, expect, vi, beforeEach } from "vitest";

// Replace the typed openapi-fetch client with a hand-rolled stub before
// importing api-client. Each method returns the {data, error, response}
// shape openapi-fetch would normally produce so parseResult can be exercised
// in isolation from the real network layer.
type StubResp<T> = { data?: T; error?: unknown; response: Response };
const stubResponses = new Map<string, StubResp<unknown>>();

function key(method: string, path: string): string {
  return `${method} ${path}`;
}

vi.mock("../../../core/contracts/workers-client", () => {
  const handler =
    (method: string) =>
    (path: string) => {
      const r = stubResponses.get(key(method, path));
      if (!r) throw new Error(`api-client.test: no stub for ${method} ${path}`);
      stubResponses.delete(key(method, path));
      return Promise.resolve(r);
    };
  return {
    client: () => ({
      GET: handler("GET"),
      POST: handler("POST"),
      DELETE: handler("DELETE"),
    }),
  };
});

import {
  detectPlatform,
  langLabel,
  langFlag,
  getUser,
  getAuthToken,
  youtubeAuthUrl,
  listSessions,
  createSession,
  getSession,
  cloneSessionVoice,
  deleteSession,
  addStream,
  removeStream,
  listVoices,
  createVoice,
  deleteVoice,
  listCredentials,
  saveCredential,
  deleteCredential,
} from "./api-client";

function stubOk<T>(method: string, path: string, body: T): void {
  stubResponses.set(key(method, path), {
    data: body,
    response: new Response(null, { status: 200 }),
  });
}

function stubFail(method: string, path: string, status: number, error?: unknown): void {
  stubResponses.set(key(method, path), {
    error,
    response: new Response(null, { status }),
  });
}

describe("api-client HTTP wrappers", () => {
  beforeEach(() => {
    stubResponses.clear();
  });

  function fullUser(overrides: Partial<Record<string, unknown>> = {}): Record<string, unknown> {
    return {
      id: "u1",
      youtube_connected: true,
      youtube_channel_name: "Aziz",
      youtube_channel_id: "UCabc",
      email: "u1@example.com",
      name: "Aziz",
      picture: null,
      onboarding_completed_at: null,
      active_voice_id: null,
      billing_tier: "self_serve",
      bills_to: null,
      created_at: 1_700_000_000,
      ...overrides,
    };
  }

  it("getUser returns the typed UserInfo (happy)", async () => {
    stubOk("GET", "/api/user", fullUser());
    const u = await getUser("u1");
    expect(u.id).toBe("u1");
    expect(u.youtube_connected).toBe(true);
    expect(u.billing_tier).toBe("self_serve");
  });

  it("getAuthToken maps {token} response", async () => {
    stubOk("POST", "/auth/token", { token: "jwt-1" });
    await expect(getAuthToken("u1")).resolves.toEqual({ token: "jwt-1" });
  });

  it("youtubeAuthUrl encodes the user_id", () => {
    expect(youtubeAuthUrl("u 1")).toContain("user_id=u%201");
  });

  it("non-2xx with parseable error body surfaces the message (sad)", async () => {
    stubFail("GET", "/api/user", 401, { error: "bad token" });
    await expect(getUser("u1")).rejects.toThrow(/API 401: bad token/);
  });

  it("non-2xx with unparseable error body falls back to generic message (sad)", async () => {
    stubFail("GET", "/api/user", 500, "garbage");
    await expect(getUser("u1")).rejects.toThrow(/API 500: request failed/);
  });

  it("listSessions returns the sessions array", async () => {
    stubOk("GET", "/api/sessions", {
      sessions: [
        {
          id: "s1",
          user_id: "u1",
          voice_id: null,
          title: "t",
          source_lang: "en",
          target_langs: "[\"ja\"]",
          status: "ready",
          live_session_id: null,
          created_at: 1,
        },
      ],
    });
    const r = await listSessions("u1");
    expect(r.sessions).toHaveLength(1);
  });

  it("createSession + getSession map streams through StreamInfo transform", async () => {
    const session = {
      id: "s1",
      user_id: "u1",
      voice_id: null,
      title: "t",
      source_lang: "en",
      target_langs: "[\"ja\"]",
      status: "ready",
      live_session_id: null,
      created_at: 1,
    };
    const stream = {
      id: "st1",
      session_id: "s1",
      lang: "ja",
      platform: "custom",
      platform_broadcast_id: "yt-bc",
      platform_stream_id: "yt-sk",
      stream_key: "k",
      rtmp_url: "rtmp://x/",
      status: "ready",
      delay_ms: 1500,
      host_gain: 0.2,
      created_at: 1,
    };
    stubOk("POST", "/api/sessions", { session, streams: [stream], errors: [] });
    const created = await createSession({
      user_id: "u1",
      title: "t",
      source_lang: "en",
      target_langs: ["ja"],
    });
    const enriched = created.streams[0] as { broadcast_id?: string; stream_id?: string };
    expect(enriched.broadcast_id).toBe("yt-bc");
    expect(enriched.stream_id).toBe("yt-sk");

    stubOk("GET", "/api/sessions/{id}", { session, streams: [stream] });
    const fetched = await getSession("s1");
    expect(fetched.streams[0].broadcast_id).toBe("yt-bc");
  });

  it("voice lifecycle (clone/list/create/delete) round-trips", async () => {
    const voice = {
      id: "v1",
      user_id: "u1",
      elevenlabs_voice_id: "el1",
      name: "n",
      source_lang: "en",
      created_at: 1,
    };
    stubOk("POST", "/api/sessions/{id}/voice", { voice });
    await expect(
      cloneSessionVoice("s1", { user_id: "u1", audio_base64: "abc" }),
    ).resolves.toEqual({ voice });

    stubOk("GET", "/api/voices", { voices: [voice] });
    await expect(listVoices("u1")).resolves.toEqual({ voices: [voice] });

    stubOk("POST", "/api/voices", voice);
    await expect(
      createVoice({ user_id: "u1", name: "n", audio_base64: "abc" }),
    ).resolves.toEqual(voice);

    stubOk("DELETE", "/api/voices/{id}", { status: "deleted" });
    await expect(deleteVoice("v1")).resolves.toEqual({ status: "deleted" });
  });

  it("session/stream/credential mutators surface server response", async () => {
    stubOk("DELETE", "/api/sessions/{id}", { status: "deleted" });
    await expect(deleteSession("s1")).resolves.toEqual({ status: "deleted" });

    stubOk("POST", "/api/sessions/{id}/streams", {
      id: "st2",
      session_id: "s1",
      lang: "zh",
      platform: "grip",
      platform_broadcast_id: null,
      platform_stream_id: null,
      stream_key: "k",
      rtmp_url: "rtmps://grip/x",
      status: "ready",
      delay_ms: 3000,
      host_gain: 0.2,
      created_at: 1,
    });
    const stream = await addStream("s1", {
      lang: "zh",
      platform: "grip",
      rtmp_url: "rtmps://grip/x",
      stream_key: "k",
    });
    expect(stream.id).toBe("st2");

    stubOk("DELETE", "/api/sessions/{session_id}/streams/{stream_id}", {
      status: "ok",
    });
    await expect(removeStream("s1", "st2")).resolves.toEqual({ status: "ok" });

    const cred = {
      id: "c1",
      user_id: "u1",
      platform: "grip",
      rtmp_url: "rtmps://grip/x",
      stream_key: "k",
      display_name: null,
      created_at: 1,
      updated_at: 1,
    };
    stubOk("GET", "/api/credentials", { credentials: [cred] });
    await expect(listCredentials("u1")).resolves.toEqual({ credentials: [cred] });

    stubOk("POST", "/api/credentials", cred);
    await expect(saveCredential({ user_id: "u1", platform: "grip" })).resolves.toEqual(cred);

    stubOk("DELETE", "/api/credentials", { status: "deleted" });
    await expect(deleteCredential("u1", "grip")).resolves.toEqual({ status: "deleted" });
  });
});


describe("detectPlatform", () => {
  it("Instagram RTMPS URL — extracts streamKey after base", () => {
    const out = detectPlatform("rtmps://live-upload.instagram.com:443/rtmp/abc123");
    expect(out).toEqual({
      platform: "instagram",
      rtmpUrl: "rtmps://live-upload.instagram.com:443/rtmp/",
      streamKey: "abc123",
    });
  });

  it("Twitch URL", () => {
    const out = detectPlatform("rtmp://live.twitch.tv/app/live_xyz");
    expect(out).toEqual({
      platform: "twitch",
      rtmpUrl: "rtmp://live.twitch.tv/app/",
      streamKey: "live_xyz",
    });
  });

  it("Kuaishou URL", () => {
    const out = detectPlatform("rtmp://live.kuaishou.com/live/key_42");
    expect(out?.platform).toBe("kuaishou");
    expect(out?.streamKey).toBe("key_42");
  });

  it("Bilibili URL", () => {
    const out = detectPlatform("rtmp://live-push.bilivideo.com/live-bvc/bili_key");
    expect(out?.platform).toBe("bilibili");
    expect(out?.streamKey).toBe("bili_key");
  });

  it("YouTube RTMP", () => {
    const out = detectPlatform("rtmp://a.rtmp.youtube.com/live2/yt_key");
    expect(out?.platform).toBe("youtube");
    expect(out?.streamKey).toBe("yt_key");
  });

  it("YouTube RTMPS", () => {
    const out = detectPlatform("rtmps://a.rtmps.youtube.com/live2/yt_s_key");
    expect(out?.platform).toBe("youtube");
    expect(out?.streamKey).toBe("yt_s_key");
  });

  it("prefix match but different base path — falls back to last segment", () => {
    const out = detectPlatform("rtmp://live.twitch.tv/different/path/k");
    expect(out?.platform).toBe("twitch");
    expect(out?.streamKey).toBe("k");
  });

  it("custom rtmp:// — splits at last slash", () => {
    const out = detectPlatform("rtmp://my.server.com/app/some_key");
    expect(out).toEqual({
      platform: "custom",
      rtmpUrl: "rtmp://my.server.com/app/",
      streamKey: "some_key",
    });
  });

  it("custom rtmps://", () => {
    const out = detectPlatform("rtmps://edge.x.io/path/abc");
    expect(out?.platform).toBe("custom");
  });

  it("trims whitespace", () => {
    const out = detectPlatform("  rtmp://live.twitch.tv/app/key  ");
    expect(out?.platform).toBe("twitch");
    expect(out?.streamKey).toBe("key");
  });

  it("empty string → null (sad)", () => {
    expect(detectPlatform("")).toBeNull();
  });

  it("whitespace-only → null (sad)", () => {
    expect(detectPlatform("   ")).toBeNull();
  });

  it("non-RTMP string → null (sad)", () => {
    expect(detectPlatform("https://youtube.com/watch?v=123")).toBeNull();
  });

  it("bare word → null (sad)", () => {
    expect(detectPlatform("justAStreamKey")).toBeNull();
  });
});

describe("langLabel", () => {
  it("known codes return label", () => {
    expect(langLabel("ko")).toBe("Korean");
    expect(langLabel("en")).toBe("English");
    expect(langLabel("ja")).toBe("Japanese");
    expect(langLabel("zh")).toBe("Chinese");
  });

  it("unknown code → returns code (fallback)", () => {
    expect(langLabel("xx")).toBe("xx");
  });

  it("empty string → empty string", () => {
    expect(langLabel("")).toBe("");
  });
});

describe("langFlag", () => {
  it("known codes return flag emoji", () => {
    expect(langFlag("ko")).toBe("\uD83C\uDDF0\uD83C\uDDF7");
    expect(langFlag("en")).toBe("\uD83C\uDDEC\uD83C\uDDE7");
    expect(langFlag("ja")).toBe("\uD83C\uDDEF\uD83C\uDDF5");
    expect(langFlag("zh")).toBe("\uD83C\uDDE8\uD83C\uDDF3");
  });

  it("unknown code → empty string (fallback)", () => {
    expect(langFlag("xx")).toBe("");
  });
});
