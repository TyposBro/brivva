import { describe, it, expect, vi, afterEach } from "vitest";
import { env } from "cloudflare:test";

import app from "../src/index";
import { verifyJwt } from "../src/auth";

async function seedUser(id: string): Promise<void> {
  await env.DB.prepare(
    "INSERT OR IGNORE INTO users (id, created_at) VALUES (?, ?)",
  )
    .bind(id, Math.floor(Date.now() / 1000))
    .run();
}

async function call(path: string, init?: RequestInit): Promise<Response> {
  const url = new URL(path, "https://test.local");
  return await app.fetch(new Request(url, init), env);
}

/**
 * Simple outbound-fetch stub. Each `expect` matches by (url substring, method)
 * and returns the given Response. First match wins. Unmatched calls throw so
 * tests can't silently rely on the real network.
 */
type StubCall = { match: RegExp; method?: string; reply: () => Response };
function installFetchStub(calls: StubCall[]): void {
  const original = globalThis.fetch;
  const used = new Set<number>();
  const stub = vi.fn(
    async (input: RequestInfo | URL, init?: RequestInit): Promise<Response> => {
      const url =
        typeof input === "string"
          ? input
          : input instanceof URL
            ? input.toString()
            : input.url;
      const method = (init?.method ?? (input instanceof Request ? input.method : "GET")).toUpperCase();
      for (let i = 0; i < calls.length; i++) {
        if (used.has(i)) continue;
        const c = calls[i];
        if (c.match.test(url) && (!c.method || c.method.toUpperCase() === method)) {
          used.add(i);
          return c.reply();
        }
      }
      throw new Error(`unstubbed fetch: ${method} ${url}`);
    },
  );
  vi.stubGlobal("fetch", stub);
  // Return the original so callers can restore. We rely on vi.unstubAllGlobals in afterEach.
  return void original;
}

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("GET /health (happy)", () => {
  it("returns {ok: true}", async () => {
    const res = await call("/health");
    expect(res.status).toBe(200);
    expect(await res.json()).toEqual({ ok: true });
  });
});

describe("OpenAPI docs", () => {
  it("GET /openapi.json returns spec with auth and session paths", async () => {
    const res = await call("/openapi.json");
    expect(res.status).toBe(200);
    const body = (await res.json()) as {
      openapi: string;
      paths: Record<string, unknown>;
    };
    expect(body.openapi).toBe("3.0.0");
    expect(body.paths["/auth/token"]).toBeTruthy();
    expect(body.paths["/api/sessions"]).toBeTruthy();
  });

  it("GET /docs returns swagger html", async () => {
    const res = await call("/docs");
    expect(res.status).toBe(200);
    const html = await res.text();
    expect(html).toContain("SwaggerUIBundle");
    expect(html).toContain("/openapi.json");
  });
});

describe("GET /api/user", () => {
  it("400 when user_id is missing (sad)", async () => {
    const res = await call("/api/user");
    expect(res.status).toBe(400);
  });

  it("creates and returns UserInfo for a new user (happy)", async () => {
    const res = await call("/api/user?user_id=fresh-1");
    expect(res.status).toBe(200);
    const body = (await res.json()) as {
      id: string;
      youtube_connected: boolean;
    };
    expect(body.id).toBe("fresh-1");
    expect(body.youtube_connected).toBe(false);
  });
});

describe("POST /api/sessions validation", () => {
  it("400 when required fields are missing (sad)", async () => {
    const res = await call("/api/sessions", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ user_id: "u-1" }),
    });
    expect(res.status).toBe(400);
  });

  it("400 when target_langs is empty (sad)", async () => {
    const res = await call("/api/sessions", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        user_id: "u-1",
        title: "T",
        source_lang: "en",
        target_langs: [],
      }),
    });
    expect(res.status).toBe(400);
  });
});

describe("POST /api/sessions — defaults + clamping", () => {
  it("uses DEFAULT_DELAY_MS=2000 when omitted (happy)", async () => {
    const res = await call("/api/sessions", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        user_id: "u-d",
        title: "Defaults",
        source_lang: "en",
        target_langs: ["ja"],
        platforms: [
          {
            platform: "twitch",
            lang: "ja",
            rtmp_url: "rtmp://tw",
            stream_key: "k1",
          },
        ],
      }),
    });
    expect(res.status).toBe(200);
    const body = (await res.json()) as {
      session: { id: string };
      streams: Array<{ delay_ms: number; host_gain: number }>;
    };
    expect(body.streams).toHaveLength(1);
    expect(body.streams[0].delay_ms).toBe(2000);
    expect(body.streams[0].host_gain).toBe(0.2); // target default
  });

  it("defaults host_gain to 1.0 when platform.lang matches source_lang (happy)", async () => {
    const res = await call("/api/sessions", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        user_id: "u-src",
        title: "Source-passthrough",
        source_lang: "en",
        target_langs: ["en"],
        platforms: [
          {
            platform: "yt",
            lang: "en",
            rtmp_url: "rtmp://yt",
            stream_key: "k",
          },
        ],
      }),
    });
    const body = (await res.json()) as {
      streams: Array<{ host_gain: number }>;
    };
    expect(body.streams[0].host_gain).toBe(1.0);
  });

  it("clamps an out-of-range host_gain into [0, 1] (edge)", async () => {
    const res = await call("/api/sessions", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        user_id: "u-clamp",
        title: "Clamp",
        source_lang: "en",
        target_langs: ["ja"],
        platforms: [
          {
            platform: "tw",
            lang: "ja",
            rtmp_url: "r",
            stream_key: "k",
            host_gain: 5.0,
          },
          {
            platform: "yt",
            lang: "ja",
            rtmp_url: "r",
            stream_key: "k",
            host_gain: -1.0,
          },
        ],
      }),
    });
    const body = (await res.json()) as {
      streams: Array<{ host_gain: number }>;
    };
    expect(body.streams[0].host_gain).toBe(1.0);
    expect(body.streams[1].host_gain).toBe(0.0);
  });

  it("silently skips platforms missing rtmp_url or stream_key (edge)", async () => {
    const res = await call("/api/sessions", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        user_id: "u-skip",
        title: "Skip",
        source_lang: "en",
        target_langs: ["ja"],
        platforms: [
          { platform: "youtube", lang: "ja" }, // auto-only, skipped
          {
            platform: "twitch",
            lang: "ja",
            rtmp_url: "rtmp://tw",
            stream_key: "k",
          },
        ],
      }),
    });
    const body = (await res.json()) as {
      streams: Array<{ platform: string }>;
    };
    expect(body.streams).toHaveLength(1);
    expect(body.streams[0].platform).toBe("twitch");
  });
});

describe("GET /api/sessions/:id", () => {
  it("returns session + its streams + joined voice (happy)", async () => {
    await seedUser("u-get");
    const createRes = await call("/api/sessions", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        user_id: "u-get",
        title: "Get-me",
        source_lang: "en",
        target_langs: ["ja"],
        platforms: [
          {
            platform: "twitch",
            lang: "ja",
            rtmp_url: "r",
            stream_key: "k",
          },
        ],
      }),
    });
    const { session } = (await createRes.json()) as {
      session: { id: string };
    };

    const getRes = await call(`/api/sessions/${session.id}`);
    expect(getRes.status).toBe(200);
    const body = (await getRes.json()) as {
      session: { id: string } | null;
      streams: Array<unknown>;
    };
    expect(body.session?.id).toBe(session.id);
    expect(body.streams).toHaveLength(1);
  });
});

describe("POST /api/sessions/:id/streams", () => {
  it("400 when required fields are missing (sad)", async () => {
    const res = await call("/api/sessions/any/streams", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ lang: "ja" }),
    });
    expect(res.status).toBe(400);
  });

  it("creates a new stream with the given delay + clamped host_gain (happy)", async () => {
    await seedUser("u-add");
    const createRes = await call("/api/sessions", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        user_id: "u-add",
        title: "A",
        source_lang: "en",
        target_langs: ["ja"],
      }),
    });
    const { session } = (await createRes.json()) as {
      session: { id: string };
    };

    const addRes = await call(`/api/sessions/${session.id}/streams`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        lang: "ja",
        platform: "twitch",
        rtmp_url: "rtmp://x",
        stream_key: "k",
        delay_ms: 3500,
        host_gain: 99, // clamped
      }),
    });
    expect(addRes.status).toBe(200);
    const body = (await addRes.json()) as {
      delay_ms: number;
      host_gain: number;
    };
    expect(body.delay_ms).toBe(3500);
    expect(body.host_gain).toBe(1.0);
  });
});

describe("POST /api/credentials", () => {
  it("upsert → upsert returns the latest row values (happy)", async () => {
    await call("/api/credentials", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        user_id: "u-cr",
        platform: "twitch",
        rtmp_url: "rtmp://a",
        stream_key: "k-old",
      }),
    });
    const res2 = await call("/api/credentials", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        user_id: "u-cr",
        platform: "twitch",
        rtmp_url: "rtmp://b",
        stream_key: "k-new",
      }),
    });
    const body = (await res2.json()) as { stream_key: string };
    expect(body.stream_key).toBe("k-new");

    const listRes = await call("/api/credentials?user_id=u-cr");
    const list = (await listRes.json()) as {
      credentials: Array<{ stream_key: string }>;
    };
    expect(list.credentials).toHaveLength(1);
    expect(list.credentials[0].stream_key).toBe("k-new");
  });
});

describe("DELETE /api/credentials", () => {
  it("requires both user_id and platform (sad)", async () => {
    const res = await call("/api/credentials?user_id=u-1", { method: "DELETE" });
    expect(res.status).toBe(400);
  });
});

describe("/internal/* authentication", () => {
  it("401 when X-Internal-Secret header is missing (sad)", async () => {
    const res = await call("/internal/users/u-anon");
    expect(res.status).toBe(401);
  });

  it("401 when X-Internal-Secret is wrong (sad)", async () => {
    const res = await call("/internal/users/u-anon", {
      headers: { "X-Internal-Secret": "not-the-secret" },
    });
    expect(res.status).toBe(401);
  });

  it("200 when X-Internal-Secret matches (happy)", async () => {
    const res = await call("/internal/users/u-ok", {
      headers: { "X-Internal-Secret": env.INTERNAL_SECRET },
    });
    expect(res.status).toBe(200);
    const body = (await res.json()) as { id: string };
    expect(body.id).toBe("u-ok");
  });

  it("GET /internal/sessions/:id returns 404 for an unknown id (sad)", async () => {
    const res = await call("/internal/sessions/does-not-exist", {
      headers: { "X-Internal-Secret": env.INTERNAL_SECRET },
    });
    expect(res.status).toBe(404);
  });

  it("PATCH /internal/sessions/:id updates the session status (happy)", async () => {
    await seedUser("u-int");
    const createRes = await call("/api/sessions", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        user_id: "u-int",
        title: "Int",
        source_lang: "en",
        target_langs: ["ja"],
      }),
    });
    const { session } = (await createRes.json()) as {
      session: { id: string };
    };

    const patch = await call(`/internal/sessions/${session.id}`, {
      method: "PATCH",
      headers: {
        "Content-Type": "application/json",
        "X-Internal-Secret": env.INTERNAL_SECRET,
      },
      body: JSON.stringify({ status: "live", live_session_id: "ROOM1" }),
    });
    expect(patch.status).toBe(200);

    const get = await call(`/internal/sessions/${session.id}`, {
      headers: { "X-Internal-Secret": env.INTERNAL_SECRET },
    });
    const body = (await get.json()) as {
      session: { status: string; live_session_id: string };
    };
    expect(body.session.status).toBe("live");
    expect(body.session.live_session_id).toBe("ROOM1");
  });
});

describe("POST /auth/token", () => {
  it("400 without user_id (sad)", async () => {
    const res = await call("/auth/token", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: "{}",
    });
    expect(res.status).toBe(400);
  });

  it("returns a JWT whose `sub` claim matches the user_id (happy)", async () => {
    const res = await call("/auth/token", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ user_id: "u-jwt" }),
    });
    expect(res.status).toBe(200);
    const body = (await res.json()) as { token: string };
    expect(body.token).toMatch(/^eyJ/);

    const claims = await verifyJwt(env.JWT_SECRET, body.token);
    expect(claims.sub).toBe("u-jwt");
  });
});

describe("GET /auth/youtube", () => {
  it("400 without user_id (sad)", async () => {
    const res = await call("/auth/youtube");
    expect(res.status).toBe(400);
  });

  it("redirects to Google with state=user_id and our redirect_uri (happy)", async () => {
    const res = await call("/auth/youtube?user_id=u-yt", {
      redirect: "manual",
    });
    expect(res.status).toBe(302);
    const location = res.headers.get("location") ?? "";
    expect(location).toContain("accounts.google.com");
    expect(location).toContain("state=u-yt");
    expect(location).toContain(encodeURIComponent(env.OAUTH_REDIRECT_URI));
  });
});

describe("GET /auth/youtube/callback (sad paths)", () => {
  it("redirects to FRONTEND_URL with oauth_error when Google signals error", async () => {
    const res = await call("/auth/youtube/callback?error=access_denied", {
      redirect: "manual",
    });
    expect(res.status).toBe(302);
    const location = res.headers.get("location") ?? "";
    expect(location.startsWith(env.FRONTEND_URL)).toBe(true);
    expect(location).toContain("oauth_error=access_denied");
  });

  it("400 when code or state is missing (sad)", async () => {
    const res = await call("/auth/youtube/callback?code=only");
    expect(res.status).toBe(400);
  });
});

describe("GET /auth/youtube/callback happy path (fetch-stubbed)", () => {
  it("exchanges code, looks up channel, stores tokens, redirects with JWT fragment", async () => {
    installFetchStub([
      {
        match: /oauth2\.googleapis\.com\/token/,
        method: "POST",
        reply: () =>
          new Response(
            JSON.stringify({
              access_token: "A-123",
              refresh_token: "R-456",
              expires_in: 3600,
              token_type: "Bearer",
              scope: "youtube",
            }),
            { status: 200, headers: { "Content-Type": "application/json" } },
          ),
      },
      {
        match: /googleapis\.com\/youtube\/v3\/channels/,
        method: "GET",
        reply: () =>
          new Response(
            JSON.stringify({
              items: [{ id: "UC-test-channel", snippet: { title: "Test Channel" } }],
            }),
            { status: 200, headers: { "Content-Type": "application/json" } },
          ),
      },
    ]);

    const res = await call(
      "/auth/youtube/callback?code=dummy-code&state=oauth-user",
      { redirect: "manual" },
    );
    expect(res.status).toBe(302);
    const location = res.headers.get("location") ?? "";
    expect(location.startsWith(env.FRONTEND_URL)).toBe(true);
    expect(location).toContain("user_id=oauth-user");
    expect(location).toMatch(/#token=eyJ/);

    // Verify D1 persisted the tokens + channel info.
    const row = (await env.DB.prepare("SELECT * FROM users WHERE id = ?")
      .bind("oauth-user")
      .first()) as {
      youtube_access_token: string;
      youtube_refresh_token: string;
      youtube_channel_id: string;
      youtube_channel_name: string;
    } | null;
    expect(row?.youtube_access_token).toBe("A-123");
    expect(row?.youtube_refresh_token).toBe("R-456");
    expect(row?.youtube_channel_id).toBe("UC-test-channel");
    expect(row?.youtube_channel_name).toBe("Test Channel");
  });
});

describe("POST /api/voices (ElevenLabs clone, fetch-mocked)", () => {
  it("400 when user_id / name / audio_base64 missing (sad)", async () => {
    const res = await call("/api/voices", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ user_id: "u-v", name: "Test" }), // audio missing
    });
    expect(res.status).toBe(400);
  });

  it("proxies the clone request + persists the returned voice_id (happy)", async () => {
    installFetchStub([
      {
        match: /api\.elevenlabs\.io\/v1\/voices\/add/,
        method: "POST",
        reply: () =>
          new Response(JSON.stringify({ voice_id: "el-cloned-123" }), {
            status: 200,
            headers: { "Content-Type": "application/json" },
          }),
      },
    ]);

    const audio = btoa("\0\0\0\0\0\0\0\0"); // 8 bytes — content ignored by the stub
    const res = await call("/api/voices", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        user_id: "u-voice",
        name: "Cloned",
        audio_base64: audio,
      }),
    });
    expect(res.status).toBe(200);
    const body = (await res.json()) as {
      elevenlabs_voice_id: string;
      user_id: string;
    };
    expect(body.elevenlabs_voice_id).toBe("el-cloned-123");
    expect(body.user_id).toBe("u-voice");

    const listRes = await call("/api/voices?user_id=u-voice");
    const list = (await listRes.json()) as {
      voices: Array<{ elevenlabs_voice_id: string }>;
    };
    expect(list.voices).toHaveLength(1);
    expect(list.voices[0].elevenlabs_voice_id).toBe("el-cloned-123");
  });

  it("does not persist a voice row when ElevenLabs rejects the clone (sad)", async () => {
    installFetchStub([
      {
        match: /api\.elevenlabs\.io\/v1\/voices\/add/,
        method: "POST",
        reply: () => new Response("quota exceeded", { status: 402 }),
      },
    ]);

    const audio = btoa("\0\0\0\0");
    const res = await call("/api/voices", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        user_id: "u-fail",
        name: "Won't clone",
        audio_base64: audio,
      }),
    });
    // Hono's default error surface is a 500 from the bubbled exception — the
    // critical assertion is that no ghost voice landed in D1.
    expect(res.ok).toBe(false);
    const listRes = await call("/api/voices?user_id=u-fail");
    const list = (await listRes.json()) as { voices: unknown[] };
    expect(list.voices).toHaveLength(0);
  });
});

describe("POST /api/sessions/:id/voice (Workers-owned session voice clone)", () => {
  it("400 when user_id or audio_base64 missing (sad)", async () => {
    const sessionRes = await call("/api/sessions", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        user_id: "u-session-voice",
        title: "Live Session",
        source_lang: "ko",
        target_langs: ["en"],
      }),
    });
    const created = (await sessionRes.json()) as {
      session: { id: string };
    };

    const res = await call(`/api/sessions/${created.session.id}/voice`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ user_id: "u-session-voice" }),
    });
    expect(res.status).toBe(400);
  });

  it("clones through Workers, persists the voice, and links it to the session (happy)", async () => {
    installFetchStub([
      {
        match: /api\.elevenlabs\.io\/v1\/voices\/add/,
        method: "POST",
        reply: () =>
          new Response(JSON.stringify({ voice_id: "el-session-voice-123" }), {
            status: 200,
            headers: { "Content-Type": "application/json" },
          }),
      },
    ]);

    const sessionRes = await call("/api/sessions", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        user_id: "u-session-voice",
        title: "Session Voice",
        source_lang: "ko",
        target_langs: ["en"],
      }),
    });
    const created = (await sessionRes.json()) as {
      session: { id: string };
    };

    const res = await call(`/api/sessions/${created.session.id}/voice`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        user_id: "u-session-voice",
        audio_base64: btoa("\0\0\0\0"),
      }),
    });
    expect(res.status).toBe(200);

    const body = (await res.json()) as {
      voice: { id: string; elevenlabs_voice_id: string; user_id: string };
    };
    expect(body.voice.user_id).toBe("u-session-voice");
    expect(body.voice.elevenlabs_voice_id).toBe("el-session-voice-123");

    const linkedSessionRes = await call(`/api/sessions/${created.session.id}`);
    const linkedSession = (await linkedSessionRes.json()) as {
      session: { voice_id: string | null };
    };
    expect(linkedSession.session.voice_id).toBe(body.voice.id);
  });

  it("403 when the caller does not own the session (sad)", async () => {
    const sessionRes = await call("/api/sessions", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        user_id: "owner-1",
        title: "Session Voice",
        source_lang: "ko",
        target_langs: ["en"],
      }),
    });
    const created = (await sessionRes.json()) as {
      session: { id: string };
    };

    const res = await call(`/api/sessions/${created.session.id}/voice`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        user_id: "other-user",
        audio_base64: btoa("\0\0\0\0"),
      }),
    });
    expect(res.status).toBe(403);
  });
});
