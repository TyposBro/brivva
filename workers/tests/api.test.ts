import { describe, it, expect, vi, afterEach } from "vitest";
import { env } from "cloudflare:test";

import app from "../src/orchestration/app";
import { verifyJwt } from "../src/shared/auth/jwt";

// Build a minimal PCM WAV of the requested duration, encoded as base64.
// 4000 Hz / 1ch / 8-bit is within the voice-sample validator and keeps the
// fixture small enough to live in memory during tests.
function makeWavBase64(seconds: number): string {
  const sampleRate = 4000;
  const channels = 1;
  const bitsPerSample = 8;
  const bytesPerSample = bitsPerSample / 8;
  const dataSize = Math.round(seconds * sampleRate * channels * bytesPerSample);
  const buf = new Uint8Array(44 + dataSize);
  const view = new DataView(buf.buffer);
  const ascii = (s: string, offset: number) => {
    for (let i = 0; i < s.length; i++) buf[offset + i] = s.charCodeAt(i);
  };
  ascii("RIFF", 0);
  view.setUint32(4, 36 + dataSize, true);
  ascii("WAVE", 8);
  ascii("fmt ", 12);
  view.setUint32(16, 16, true); // fmt chunk size
  view.setUint16(20, 1, true); // PCM
  view.setUint16(22, channels, true);
  view.setUint32(24, sampleRate, true);
  view.setUint32(28, sampleRate * channels * bytesPerSample, true); // byteRate
  view.setUint16(32, channels * bytesPerSample, true); // blockAlign
  view.setUint16(34, bitsPerSample, true);
  ascii("data", 36);
  view.setUint32(40, dataSize, true);
  // data stays zero-filled (silence) — content is irrelevant for these tests.
  let binary = "";
  for (let i = 0; i < buf.length; i++) binary += String.fromCharCode(buf[i]!);
  return btoa(binary);
}

const VALID_WAV_B64 = makeWavBase64(32);

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

describe("GET / root handler", () => {
  it("identifies the worker (happy)", async () => {
    const res = await call("/");
    expect(res.status).toBe(200);
    const body = await res.text();
    expect(body).toContain("Brivva API");
  });
});

describe("OpenAPI docs", () => {
  it("GET /openapi.json returns spec with auth and session paths", async () => {
    const res = await call("/openapi.json");
    expect(res.status).toBe(200);
    const body = (await res.json()) as {
      openapi: string;
      info: { version: string };
      paths: Record<string, unknown>;
    };
    expect(body.openapi).toBe("3.0.0");
    expect(body.paths["/auth/token"]).toBeTruthy();
    expect(body.paths["/api/sessions"]).toBeTruthy();
    expect(body.paths["/internal/sessions/{id}"]).toBeTruthy();
    expect(body.info.version).toBe("0.2.0");
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

describe("POST /api/sessions — voice source_lang guard", () => {
  // Cross-lingual TTS is handled by ElevenLabs `language_code` on the TARGET
  // side. The enrollment language must match what the host actually speaks,
  // otherwise the clone re-synthesizes with the wrong accent (the Apr 2026
  // "Indian accent" regression that prompted this strict guard). Targets stay
  // flexible — only source_lang is pinned.

  it("allows session create when user has no voice clone (happy)", async () => {
    // No voice → nothing to mismatch. Default voice has no enrollment lang,
    // so the session proceeds normally.
    await seedUser("u-voiceless");
    const res = await call("/api/sessions", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        user_id: "u-voiceless",
        title: "No clone",
        source_lang: "ko",
        target_langs: ["en"],
      }),
    });
    expect(res.status).toBe(200);
  });

  it("allows session create when voice.source_lang matches session.source_lang (happy)", async () => {
    await seedUser("u-match");
    await env.DB.prepare(
      "INSERT INTO voices (id, user_id, elevenlabs_voice_id, name, source_lang, created_at) VALUES (?, ?, ?, ?, ?, ?)",
    )
      .bind("v-match", "u-match", "el-match", "Aziz", "ko", Math.floor(Date.now() / 1000))
      .run();
    await env.DB.prepare("UPDATE users SET active_voice_id = ? WHERE id = ?")
      .bind("v-match", "u-match")
      .run();

    const res = await call("/api/sessions", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        user_id: "u-match",
        title: "Match",
        source_lang: "ko",
        target_langs: ["ja", "en"],
      }),
    });
    expect(res.status).toBe(200);
  });

  it("rejects session create when voice.source_lang differs from session.source_lang (sad)", async () => {
    await seedUser("u-mismatch");
    await env.DB.prepare(
      "INSERT INTO voices (id, user_id, elevenlabs_voice_id, name, source_lang, created_at) VALUES (?, ?, ?, ?, ?, ?)",
    )
      .bind("v-mismatch", "u-mismatch", "el-mismatch", "Aziz", "en", Math.floor(Date.now() / 1000))
      .run();
    await env.DB.prepare("UPDATE users SET active_voice_id = ? WHERE id = ?")
      .bind("v-mismatch", "u-mismatch")
      .run();

    const res = await call("/api/sessions", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        user_id: "u-mismatch",
        title: "Mismatch",
        source_lang: "ko",
        target_langs: ["ja"],
      }),
    });
    expect(res.status).toBe(400);
    const body = (await res.json()) as {
      error: string;
      voice_source_lang: string;
      session_source_lang: string;
    };
    expect(body.error).toBe("voice_language_mismatch");
    expect(body.voice_source_lang).toBe("en");
    expect(body.session_source_lang).toBe("ko");

    // Reject must NOT create a session row (no side effects before the guard).
    const sessionsRes = await call("/api/sessions?user_id=u-mismatch");
    const sessionsBody = (await sessionsRes.json()) as {
      sessions: Array<{ id: string }>;
    };
    expect(sessionsBody.sessions).toHaveLength(0);
  });

  it("treats a legacy voice with null source_lang as mismatch (sad)", async () => {
    // Pre-migration 0006 rows have source_lang=NULL. Without a known
    // enrollment lang we can't prove a match, so force re-record.
    await seedUser("u-legacy");
    await env.DB.prepare(
      "INSERT INTO voices (id, user_id, elevenlabs_voice_id, name, source_lang, created_at) VALUES (?, ?, ?, ?, ?, ?)",
    )
      .bind("v-legacy", "u-legacy", "el-legacy", "Aziz", null, Math.floor(Date.now() / 1000))
      .run();
    await env.DB.prepare("UPDATE users SET active_voice_id = ? WHERE id = ?")
      .bind("v-legacy", "u-legacy")
      .run();

    const res = await call("/api/sessions", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        user_id: "u-legacy",
        title: "Legacy",
        source_lang: "ko",
        target_langs: ["en"],
      }),
    });
    expect(res.status).toBe(400);
    const body = (await res.json()) as {
      error: string;
      voice_source_lang: string | null;
    };
    expect(body.error).toBe("voice_language_mismatch");
    expect(body.voice_source_lang).toBeNull();
  });
});

describe("POST /api/sessions — passthrough destinations", () => {
  it("accepts lang=pass sentinel in target_langs + platforms (happy)", async () => {
    // Passthrough destinations bypass STT/translate/TTS on Fargate. Workers
    // doesn't interpret the sentinel — it just persists it. The pipeline
    // layer reads `streams.lang == "pass"` to switch into passthrough mode.
    const res = await call("/api/sessions", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        user_id: "u-pass",
        title: "Passthrough",
        source_lang: "en",
        target_langs: ["ja", "pass"],
        platforms: [
          {
            platform: "tw",
            lang: "ja",
            rtmp_url: "rtmp://tw-ja",
            stream_key: "ja-key",
          },
          {
            platform: "yt",
            lang: "pass",
            rtmp_url: "rtmp://yt-pass",
            stream_key: "pass-key",
          },
        ],
      }),
    });
    expect(res.status).toBe(200);
    const body = (await res.json()) as {
      streams: Array<{ lang: string; host_gain: number }>;
    };
    expect(body.streams).toHaveLength(2);
    const byLang = Object.fromEntries(body.streams.map((s) => [s.lang, s]));
    expect(byLang["ja"]).toBeDefined();
    expect(byLang["pass"]).toBeDefined();
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
          { platform: "youtube", lang: "ja" }, // no YT token → skipped
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

// ── YouTube auto-fill (broadcast-create) ─────────────────
// When a user adds a YouTube destination and has an OAuth access_token in
// users, Workers creates the broadcast + stream + binds them, populates the
// D1 streams row with rtmp_url/stream_key/platform_broadcast_id, and returns
// a watch_url so the FE can surface a shareable link.

async function seedYoutubeUser(
  id: string,
  opts: { accessToken?: string; refreshToken?: string; expiresAt?: number } = {},
): Promise<void> {
  await seedUser(id);
  const access = opts.accessToken ?? "yt-access-token";
  const refresh = opts.refreshToken ?? "yt-refresh-token";
  const expires = opts.expiresAt ?? Math.floor(Date.now() / 1000) + 3600;
  await env.DB.prepare(
    `UPDATE users SET
       youtube_access_token = ?,
       youtube_refresh_token = ?,
       youtube_token_expires_at = ?,
       youtube_channel_id = ?,
       youtube_channel_name = ?
     WHERE id = ?`,
  )
    .bind(access, refresh, expires, "UC-test", "Test Channel", id)
    .run();
}

function jsonReply(body: unknown, status = 200): () => Response {
  return () =>
    new Response(JSON.stringify(body), {
      status,
      headers: { "Content-Type": "application/json" },
    });
}

describe("POST /api/sessions — YouTube auto-fill", () => {
  it("auto-creates broadcast + stream + bind; populates rtmp_url + watch_url (happy)", async () => {
    await seedYoutubeUser("u-yt-happy");

    installFetchStub([
      {
        match: /liveBroadcasts\?/,
        method: "POST",
        reply: jsonReply({ id: "bcast-1" }),
      },
      {
        match: /liveStreams\?/,
        method: "POST",
        reply: jsonReply({
          id: "stream-1",
          cdn: {
            ingestionInfo: {
              ingestionAddress: "rtmp://a.rtmp.youtube.com/live2",
              streamName: "k-abc",
            },
          },
        }),
      },
      {
        match: /liveBroadcasts\/bind/,
        method: "POST",
        reply: jsonReply({ id: "bcast-1" }),
      },
    ]);

    const res = await call("/api/sessions", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        user_id: "u-yt-happy",
        title: "My Live Show",
        source_lang: "en",
        target_langs: ["ja"],
        privacy_status: "unlisted",
        platforms: [{ platform: "youtube", lang: "ja" }],
      }),
    });

    expect(res.status).toBe(200);
    const body = (await res.json()) as {
      session: { id: string };
      streams: Array<{
        platform: string;
        rtmp_url: string | null;
        stream_key: string | null;
        platform_broadcast_id: string | null;
        platform_stream_id: string | null;
        watch_url?: string | null;
      }>;
    };
    expect(body.streams).toHaveLength(1);
    const s = body.streams[0]!;
    expect(s.platform).toBe("youtube");
    expect(s.rtmp_url).toBe("rtmp://a.rtmp.youtube.com/live2");
    expect(s.stream_key).toBe("k-abc");
    expect(s.platform_broadcast_id).toBe("bcast-1");
    expect(s.platform_stream_id).toBe("stream-1");
    expect(s.watch_url).toBe("https://www.youtube.com/watch?v=bcast-1");

    // D1 row must persist the populated values (Fargate will read it later).
    const row = await env.DB.prepare(
      "SELECT rtmp_url, stream_key, platform_broadcast_id FROM streams WHERE session_id = ?",
    )
      .bind(body.session.id)
      .first<{
        rtmp_url: string;
        stream_key: string;
        platform_broadcast_id: string;
      }>();
    expect(row?.rtmp_url).toBe("rtmp://a.rtmp.youtube.com/live2");
    expect(row?.stream_key).toBe("k-abc");
    expect(row?.platform_broadcast_id).toBe("bcast-1");
  });

  it("refreshes an expired OAuth token before retrying broadcast-create (edge)", async () => {
    await seedYoutubeUser("u-yt-refresh", {
      accessToken: "expired-token",
      refreshToken: "rtok",
    });

    installFetchStub([
      // First attempt → 401 (access token expired at YouTube's end).
      {
        match: /liveBroadcasts\?/,
        method: "POST",
        reply: () => new Response("unauthorized", { status: 401 }),
      },
      // OAuth refresh call.
      {
        match: /oauth2\.googleapis\.com\/token/,
        method: "POST",
        reply: jsonReply({ access_token: "new-token", expires_in: 3600 }),
      },
      // Retry succeeds.
      {
        match: /liveBroadcasts\?/,
        method: "POST",
        reply: jsonReply({ id: "bcast-2" }),
      },
      {
        match: /liveStreams\?/,
        method: "POST",
        reply: jsonReply({
          id: "stream-2",
          cdn: {
            ingestionInfo: {
              ingestionAddress: "rtmp://a.rtmp.youtube.com/live2",
              streamName: "k-2",
            },
          },
        }),
      },
      {
        match: /liveBroadcasts\/bind/,
        method: "POST",
        reply: jsonReply({ id: "bcast-2" }),
      },
    ]);

    const res = await call("/api/sessions", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        user_id: "u-yt-refresh",
        title: "Refresh test",
        source_lang: "en",
        target_langs: ["ja"],
        platforms: [{ platform: "youtube", lang: "ja" }],
      }),
    });
    expect(res.status).toBe(200);

    // Refreshed access token must persist to D1 so the next session
    // doesn't need to refresh again.
    const user = await env.DB.prepare(
      "SELECT youtube_access_token FROM users WHERE id = ?",
    )
      .bind("u-yt-refresh")
      .first<{ youtube_access_token: string }>();
    expect(user?.youtube_access_token).toBe("new-token");
  });

  it("cleans up session + streams when YouTube returns 403 (sad)", async () => {
    await seedYoutubeUser("u-yt-403");

    installFetchStub([
      {
        match: /liveBroadcasts\?/,
        method: "POST",
        reply: () => new Response("quotaExceeded", { status: 403 }),
      },
    ]);

    const res = await call("/api/sessions", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        user_id: "u-yt-403",
        title: "403 test",
        source_lang: "en",
        target_langs: ["ja"],
        platforms: [{ platform: "youtube", lang: "ja" }],
      }),
    });
    expect(res.status).toBe(403);
    const body = (await res.json()) as { error: string };
    expect(body.error).toMatch(/YouTube broadcast create failed/);
    expect(body.error).toMatch(/quotaExceeded/);

    // No orphan rows — session + streams must be gone.
    const sessionCount = await env.DB.prepare(
      "SELECT COUNT(*) AS c FROM sessions WHERE user_id = ?",
    )
      .bind("u-yt-403")
      .first<{ c: number }>();
    expect(sessionCount?.c).toBe(0);
    const streamCount = await env.DB.prepare(
      "SELECT COUNT(*) AS c FROM streams",
    ).first<{ c: number }>();
    expect(streamCount?.c).toBe(0);
  });

  it("rolls back an already-inserted manual stream when a later YouTube call fails (edge)", async () => {
    await seedYoutubeUser("u-yt-mix");

    installFetchStub([
      {
        match: /liveBroadcasts\?/,
        method: "POST",
        reply: () => new Response("permissionDenied", { status: 403 }),
      },
    ]);

    const res = await call("/api/sessions", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        user_id: "u-yt-mix",
        title: "Mixed",
        source_lang: "en",
        target_langs: ["ja"],
        platforms: [
          {
            platform: "twitch",
            lang: "ja",
            rtmp_url: "rtmp://tw",
            stream_key: "k-tw",
          },
          { platform: "youtube", lang: "ja" },
        ],
      }),
    });
    expect(res.status).toBe(403);

    // The twitch row that landed first must be cleaned up along with the
    // failed session — otherwise `POST /api/sessions` ends up with half-baked
    // D1 state.
    const streamCount = await env.DB.prepare(
      "SELECT COUNT(*) AS c FROM streams",
    ).first<{ c: number }>();
    expect(streamCount?.c).toBe(0);
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

describe("DELETE /api/sessions/:id soft-ends the session (happy)", () => {
  it("DELETE marks status='ended' and keeps the row + streams reachable so the summary view can render", async () => {
    await seedUser("u-del-s");
    const createRes = await call("/api/sessions", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        user_id: "u-del-s",
        title: "D",
        source_lang: "en",
        target_langs: ["ja"],
        platforms: [
          { platform: "tw", lang: "ja", rtmp_url: "rtmp://x", stream_key: "k" },
        ],
      }),
    });
    const { session, streams } = (await createRes.json()) as {
      session: { id: string };
      streams: Array<{ id: string }>;
    };

    // Stream-level deletes are still hard deletes — the row is per-destination
    // not per-session, and the host can prune destinations mid-setup. Keep
    // that path as-is and assert it.
    const delStreamRes = await call(
      `/api/sessions/${session.id}/streams/${streams[0]!.id}`,
      { method: "DELETE" },
    );
    expect(delStreamRes.status).toBe(200);

    const delSessionRes = await call(`/api/sessions/${session.id}`, {
      method: "DELETE",
    });
    expect(delSessionRes.status).toBe(200);
    const delBody = (await delSessionRes.json()) as { status: string };
    expect(delBody.status).toBe("ended");

    // GET still returns 200 with the row — status flipped to 'ended'. This
    // is the contract the FE relies on to render the post-stream summary
    // instead of "Session not found" (prod 2026-04-20 incident).
    const getRes = await call(`/api/sessions/${session.id}`);
    expect(getRes.status).toBe(200);
    const getBody = (await getRes.json()) as {
      session: { id: string; status: string } | null;
    };
    expect(getBody.session).not.toBeNull();
    expect(getBody.session!.id).toBe(session.id);
    expect(getBody.session!.status).toBe("ended");
  });

  it("DELETE on a missing session returns 200 (idempotent — server-rs may race the FE)", async () => {
    const res = await call("/api/sessions/does-not-exist", { method: "DELETE" });
    expect(res.status).toBe(200);
    const body = (await res.json()) as { status: string };
    expect(body.status).toBe("ended");
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

  it("deletes the matching row (happy)", async () => {
    await call("/api/credentials", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        user_id: "u-delcr",
        platform: "twitch",
        rtmp_url: "rtmp://x",
        stream_key: "k",
      }),
    });
    const res = await call("/api/credentials?user_id=u-delcr&platform=twitch", {
      method: "DELETE",
    });
    expect(res.status).toBe(200);
    const list = await call("/api/credentials?user_id=u-delcr");
    const body = (await list.json()) as { credentials: unknown[] };
    expect(body.credentials).toHaveLength(0);
  });
});

describe("GET /api/sessions list", () => {
  it("returns user's sessions newest first (happy)", async () => {
    await seedUser("u-list");
    await call("/api/sessions", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        user_id: "u-list",
        title: "first",
        source_lang: "en",
        target_langs: ["ja"],
      }),
    });
    const res = await call("/api/sessions?user_id=u-list");
    expect(res.status).toBe(200);
    const body = (await res.json()) as { sessions: Array<{ title: string }> };
    expect(body.sessions.length).toBeGreaterThanOrEqual(1);
  });

  it("400 without user_id (sad)", async () => {
    const res = await call("/api/sessions");
    expect(res.status).toBe(400);
  });
});

describe("DELETE /api/voices/:id", () => {
  it("404 when voice not found (sad)", async () => {
    const res = await call("/api/voices/not-real", { method: "DELETE" });
    expect(res.status).toBe(404);
  });

  it("clears active_voice_id when deleting the currently-active voice (happy)", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
        const url =
          typeof input === "string"
            ? input
            : input instanceof URL
              ? input.toString()
              : input.url;
        const method = (
          init?.method ?? (input instanceof Request ? input.method : "GET")
        ).toUpperCase();
        if (/voices\/add/.test(url)) {
          return new Response(JSON.stringify({ voice_id: "el-del" }), { status: 200 });
        }
        if (method === "DELETE") return new Response("", { status: 200 });
        throw new Error(`unstubbed: ${method} ${url}`);
      }),
    );
    const created = await call("/api/voices", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        user_id: "u-voicedel",
        name: "V",
        audio_base64: VALID_WAV_B64,
      }),
    });
    const voice = (await created.json()) as { id: string };

    const del = await call(`/api/voices/${voice.id}`, { method: "DELETE" });
    expect(del.status).toBe(200);

    const user = await call("/api/user?user_id=u-voicedel");
    const userBody = (await user.json()) as { active_voice_id: string | null };
    expect(userBody.active_voice_id).toBeNull();
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

  it("GET /internal/voices/:id returns the voice row when authed (happy)", async () => {
    await env.DB.prepare(
      "INSERT INTO users (id, created_at) VALUES (?, ?)",
    ).bind("u-iv", Math.floor(Date.now() / 1000)).run();
    const id = crypto.randomUUID();
    await env.DB.prepare(
      "INSERT INTO voices (id, user_id, elevenlabs_voice_id, name, created_at) VALUES (?, ?, ?, ?, ?)",
    ).bind(id, "u-iv", "el-internal", "V", Math.floor(Date.now() / 1000)).run();

    const res = await call(`/internal/voices/${id}`, {
      headers: { "X-Internal-Secret": env.INTERNAL_SECRET },
    });
    expect(res.status).toBe(200);
    const body = (await res.json()) as { elevenlabs_voice_id: string };
    expect(body.elevenlabs_voice_id).toBe("el-internal");
  });

  it("GET /internal/voices/:id → 401 without secret (sad)", async () => {
    const res = await call("/internal/voices/anything");
    expect(res.status).toBe(401);
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

describe("GET /auth/youtube/callback 500 path", () => {
  it("returns 500 when the token exchange throws (sad)", async () => {
    installFetchStub([
      {
        match: /oauth2\.googleapis\.com\/token/,
        method: "POST",
        reply: () => new Response("denied", { status: 500 }),
      },
    ]);
    const res = await call("/auth/youtube/callback?code=x&state=u", {
      redirect: "manual",
    });
    expect(res.status).toBe(500);
    const body = (await res.json()) as { error: string };
    expect(body.error).toMatch(/OAuth token exchange/);
  });
});

describe("GET /auth/google/callback 500 path", () => {
  it("returns 500 when userinfo throws (sad)", async () => {
    installFetchStub([
      {
        match: /oauth2\.googleapis\.com\/token/,
        method: "POST",
        reply: () =>
          new Response(
            JSON.stringify({ access_token: "A", expires_in: 60, token_type: "Bearer", scope: "o" }),
            { status: 200 },
          ),
      },
      {
        match: /openidconnect\.googleapis\.com\/v1\/userinfo/,
        method: "GET",
        reply: () => new Response("nope", { status: 401 }),
      },
    ]);
    const res = await call("/auth/google/callback?code=x&state=s", {
      redirect: "manual",
    });
    expect(res.status).toBe(500);
  });

  it("400 when code missing but no error param (sad)", async () => {
    const res = await call("/auth/google/callback?state=s");
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

    const audio = VALID_WAV_B64;
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

    const audio = VALID_WAV_B64;
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
        audio_base64: VALID_WAV_B64,
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

  it("rejects voice samples shorter than 30 seconds (sad)", async () => {
    const sessionRes = await call("/api/sessions", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        user_id: "u-short",
        title: "Short",
        source_lang: "ko",
        target_langs: ["en"],
      }),
    });
    const { session } = (await sessionRes.json()) as { session: { id: string } };
    const res = await call(`/api/sessions/${session.id}/voice`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        user_id: "u-short",
        audio_base64: makeWavBase64(10), // too short
      }),
    });
    expect(res.status).toBe(400);
    const body = (await res.json()) as { error: string };
    expect(body.error).toMatch(/too short/);
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
        audio_base64: VALID_WAV_B64,
      }),
    });
    expect(res.status).toBe(403);
  });

  it("forwards source_lang to ElevenLabs as labels + persists on voice row (happy)", async () => {
    let seenLabels: string | null = null;
    vi.stubGlobal(
      "fetch",
      vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
        const url =
          typeof input === "string"
            ? input
            : input instanceof URL
              ? input.toString()
              : input.url;
        if (/api\.elevenlabs\.io\/v1\/voices\/add/.test(url)) {
          const form = init?.body as FormData;
          seenLabels = (form.get("labels") as string | null) ?? null;
          return new Response(JSON.stringify({ voice_id: "el-lang-1" }), {
            status: 200,
            headers: { "Content-Type": "application/json" },
          });
        }
        throw new Error(`unstubbed fetch: ${url}`);
      }),
    );

    const sessionRes = await call("/api/sessions", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        user_id: "u-lang",
        title: "Lang Session",
        source_lang: "ko",
        target_langs: ["en"],
      }),
    });
    const { session } = (await sessionRes.json()) as { session: { id: string } };
    const res = await call(`/api/sessions/${session.id}/voice`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        user_id: "u-lang",
        audio_base64: VALID_WAV_B64,
        source_lang: "ko",
      }),
    });
    expect(res.status).toBe(200);
    expect(seenLabels).toBe(JSON.stringify({ language: "ko" }));

    const body = (await res.json()) as { voice: { source_lang: string | null } };
    expect(body.voice.source_lang).toBe("ko");
  });
});

describe("POST /api/voices — source_lang plumbing", () => {
  it("forwards source_lang to ElevenLabs labels + persists on row (happy)", async () => {
    let seenLabels: string | null = null;
    vi.stubGlobal(
      "fetch",
      vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
        const url =
          typeof input === "string"
            ? input
            : input instanceof URL
              ? input.toString()
              : input.url;
        if (/api\.elevenlabs\.io\/v1\/voices\/add/.test(url)) {
          const form = init?.body as FormData;
          seenLabels = (form.get("labels") as string | null) ?? null;
          return new Response(JSON.stringify({ voice_id: "el-ko-1" }), {
            status: 200,
            headers: { "Content-Type": "application/json" },
          });
        }
        throw new Error(`unstubbed fetch: ${url}`);
      }),
    );

    const res = await call("/api/voices", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        user_id: "u-lang-v",
        name: "Ko voice",
        audio_base64: VALID_WAV_B64,
        source_lang: "ko",
      }),
    });
    expect(res.status).toBe(200);
    const body = (await res.json()) as { source_lang: string | null };
    expect(body.source_lang).toBe("ko");
    expect(seenLabels).toBe(JSON.stringify({ language: "ko" }));
  });
});

describe("POST /api/voices rejects short samples (sad)", () => {
  it("400 when audio_base64 is a <30s WAV", async () => {
    const res = await call("/api/voices", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        user_id: "u-tiny",
        name: "Tiny",
        audio_base64: makeWavBase64(5),
      }),
    });
    expect(res.status).toBe(400);
    const body = (await res.json()) as { error: string };
    expect(body.error).toMatch(/too short/);
  });
});

describe("GET /auth/google (happy)", () => {
  it("redirects to Google with openid+email+profile scope", async () => {
    const res = await call("/auth/google", { redirect: "manual" });
    expect(res.status).toBe(302);
    const location = res.headers.get("location") ?? "";
    expect(location).toContain("accounts.google.com");
    expect(location).toContain("scope=openid+email+profile");
    expect(location).toContain(encodeURIComponent(env.GOOGLE_SIGNIN_REDIRECT_URI));
  });
});

describe("GET /auth/google/callback (fetch-stubbed, happy)", () => {
  it("mints JWT by Google sub + persists email/name/picture", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async (input: RequestInfo | URL) => {
        const url =
          typeof input === "string"
            ? input
            : input instanceof URL
              ? input.toString()
              : input.url;
        if (/oauth2\.googleapis\.com\/token/.test(url)) {
          return new Response(
            JSON.stringify({
              access_token: "goog-access",
              expires_in: 3600,
              token_type: "Bearer",
              scope: "openid email profile",
            }),
            { status: 200, headers: { "Content-Type": "application/json" } },
          );
        }
        if (/openidconnect\.googleapis\.com\/v1\/userinfo/.test(url)) {
          return new Response(
            JSON.stringify({
              sub: "google-sub-123",
              email: "host@example.com",
              name: "Host Name",
              picture: "https://cdn.example.com/host.png",
            }),
            { status: 200, headers: { "Content-Type": "application/json" } },
          );
        }
        throw new Error(`unstubbed fetch: ${url}`);
      }),
    );

    const res = await call("/auth/google/callback?code=auth-code&state=any", {
      redirect: "manual",
    });
    expect(res.status).toBe(302);
    const location = res.headers.get("location") ?? "";
    expect(location).toContain("user_id=google-sub-123");
    expect(location).toMatch(/#token=eyJ/);

    const jwt = /#token=([^&]+)/.exec(location)![1]!;
    const claims = await verifyJwt(env.JWT_SECRET, jwt);
    expect(claims.sub).toBe("google-sub-123");

    const row = (await env.DB.prepare("SELECT * FROM users WHERE id = ?")
      .bind("google-sub-123")
      .first()) as {
      email: string;
      name: string;
      picture: string;
    } | null;
    expect(row?.email).toBe("host@example.com");
    expect(row?.name).toBe("Host Name");
    expect(row?.picture).toBe("https://cdn.example.com/host.png");
  });

  it("redirects with oauth_error when Google signals error (sad)", async () => {
    const res = await call("/auth/google/callback?error=access_denied", {
      redirect: "manual",
    });
    expect(res.status).toBe(302);
    const location = res.headers.get("location") ?? "";
    expect(location.startsWith(env.FRONTEND_URL)).toBe(true);
    expect(location).toContain("oauth_error=access_denied");
  });
});

describe("POST /auth/grip + /auth/tiktok", () => {
  it("410 Gone on POST /auth/grip — keys are one-shot, save path removed", async () => {
    // Grip stream keys are one-shot per broadcast (AWS IVS rejects duplicate
    // publishers). Saving them guaranteed the failure where the second
    // session died silently after ~25s. The endpoint now refuses to persist
    // so a legacy client can't reintroduce the bug.
    const res = await call("/auth/grip", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        user_id: "u-grip",
        session_token: "sess-abc-def",
        stream_key: "grip-stream-key",
        rtmp_url: "rtmps://live.grip.fans:443/live/",
        display_name: "Grip main",
      }),
    });
    expect(res.status).toBe(410);
    const body = (await res.json()) as { error: string; message: string };
    expect(body.error).toBe("grip_creds_not_savable");
    expect(body.message).toMatch(/one-shot/i);
  });

  it("400 when required fields missing on /auth/tiktok (sad)", async () => {
    const res = await call("/auth/tiktok", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ user_id: "u-t", session_token: "x" }), // no stream_key
    });
    expect(res.status).toBe(400);
  });

  it("upserts a TikTok credential row (happy)", async () => {
    const res = await call("/auth/tiktok", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        user_id: "u-tt",
        session_token: "tiktok-session-xxx",
        stream_key: "tiktok-key",
      }),
    });
    expect(res.status).toBe(200);
    const body = (await res.json()) as { platform: string; display_name: string };
    expect(body.platform).toBe("tiktok");
    expect(body.display_name).toContain("tiktok:");
  });
});

// Paste-creds round-trip coverage. These tests assert the full lifecycle for
// both Grip and TikTok: write via /auth/<platform>, read back via
// GET /api/credentials, validation rejects on missing fields, and repeated
// writes for the same (user_id, platform) UPSERT rather than duplicate.
// The Grip and TikTok flows share the schema shape but live under separate
// describe blocks so regressions in one platform can't silently pass by
// inheriting the other's assertions.

describe("POST /auth/grip — save path removed (ephemeral one-shot keys)", () => {
  // These four tests were originally a write → list round-trip (commit
  // 3f24189). The save path has been deleted because Grip stream keys are
  // one-shot per broadcast (AWS IVS under the hood: a fresh key issues ~1h
  // before each show, and IVS rejects a duplicate publisher — the second
  // session connects briefly then dies silently after ~25s). Every request
  // shape that previously worked must now be rejected with 410, and nothing
  // should ever hit platform_credentials.

  it("410 on a fully-valid Grip save request (what used to be the happy path)", async () => {
    const res = await call("/auth/grip", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        user_id: "u-grip-rt",
        session_token: "sess-1234567890",
        stream_key: "grip-sk-aaa",
        rtmp_url: "rtmps://live.grip.fans:443/live/",
      }),
    });
    expect(res.status).toBe(410);
    const body = (await res.json()) as { error: string; message: string };
    expect(body.error).toBe("grip_creds_not_savable");

    // Critical: no D1 row may exist. If the handler accidentally persisted
    // before returning the error, the FE's defensive filter would hide it
    // — but a future migration could re-surface it. Pin the contract here.
    const dbRow = (await env.DB.prepare(
      "SELECT stream_key FROM platform_credentials WHERE user_id = ? AND platform = ?",
    )
      .bind("u-grip-rt", "grip")
      .first()) as { stream_key: string } | null;
    expect(dbRow).toBeNull();

    // And GET /api/credentials for that user returns nothing Grip-shaped.
    const listRes = await call("/api/credentials?user_id=u-grip-rt");
    expect(listRes.status).toBe(200);
    const list = (await listRes.json()) as {
      credentials: Array<{ platform: string }>;
    };
    expect(list.credentials.find((c) => c.platform === "grip")).toBeUndefined();
  });

  it("410 even on a malformed body — no validation branch matters anymore", async () => {
    // Previously this shape returned 400 (stream_key empty). The new handler
    // short-circuits before any schema check, so every body is 410.
    const res = await call("/auth/grip", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        user_id: "u-grip-bad",
        session_token: "sess-abc",
        stream_key: "",
      }),
    });
    expect(res.status).toBe(410);
    const body = (await res.json()) as { error: string };
    expect(body.error).toBe("grip_creds_not_savable");
  });

  it("410 when user_id is missing — previously 400, now uniformly rejected", async () => {
    const res = await call("/auth/grip", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        session_token: "sess-abc",
        stream_key: "k",
      }),
    });
    expect(res.status).toBe(410);
  });

  it("repeated calls never produce a row — replaces the old UPSERT test", async () => {
    // The old test asserted two saves collapse into one row via UPSERT. The
    // new contract is stronger: zero rows, no matter how many times the FE
    // (or a stale client) retries.
    await call("/auth/grip", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        user_id: "u-grip-upsert",
        session_token: "sess-1",
        stream_key: "first-key",
        rtmp_url: "rtmps://a.example/live/",
      }),
    });
    const res2 = await call("/auth/grip", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        user_id: "u-grip-upsert",
        session_token: "sess-2",
        stream_key: "second-key",
        rtmp_url: "rtmps://b.example/live/",
      }),
    });
    expect(res2.status).toBe(410);

    const rows = await env.DB.prepare(
      "SELECT stream_key FROM platform_credentials WHERE user_id = ? AND platform = ?",
    )
      .bind("u-grip-upsert", "grip")
      .all();
    expect(rows.results).toHaveLength(0);
  });
});

describe("POST /auth/tiktok — paste-creds round-trip", () => {
  // Mirrors the Grip round-trip. Keeping the assertions separate — instead of
  // parametrizing — guards against a platform-specific regression slipping by
  // just because the other platform still works.

  it("persists the row and exposes it via GET /api/credentials (happy)", async () => {
    const res = await call("/auth/tiktok", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        user_id: "u-tt-rt",
        session_token: "tt-sess-xyz",
        stream_key: "tt-sk-aaa",
        rtmp_url: "rtmp://live.tiktok.example/live/",
      }),
    });
    expect(res.status).toBe(200);
    const body = (await res.json()) as {
      platform: string;
      stream_key: string;
      rtmp_url: string | null;
    };
    expect(body.platform).toBe("tiktok");
    expect(body.stream_key).toBe("tt-sk-aaa");

    const dbRow = (await env.DB.prepare(
      "SELECT stream_key, rtmp_url FROM platform_credentials WHERE user_id = ? AND platform = ?",
    )
      .bind("u-tt-rt", "tiktok")
      .first()) as { stream_key: string; rtmp_url: string } | null;
    expect(dbRow).not.toBeNull();
    expect(dbRow!.stream_key).toBe("tt-sk-aaa");
    expect(dbRow!.rtmp_url).toBe("rtmp://live.tiktok.example/live/");

    const listRes = await call("/api/credentials?user_id=u-tt-rt");
    const list = (await listRes.json()) as {
      credentials: Array<{ platform: string; stream_key: string }>;
    };
    const ttRow = list.credentials.find((c) => c.platform === "tiktok");
    expect(ttRow).toBeDefined();
    expect(ttRow!.stream_key).toBe("tt-sk-aaa");
  });

  it("400 when stream_key is empty (sad)", async () => {
    const res = await call("/auth/tiktok", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        user_id: "u-tt-bad",
        session_token: "sess",
        stream_key: "",
      }),
    });
    expect(res.status).toBe(400);
  });

  it("400 when user_id is missing (sad)", async () => {
    const res = await call("/auth/tiktok", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        session_token: "sess",
        stream_key: "k",
      }),
    });
    expect(res.status).toBe(400);
  });

  it("repeated saves UPSERT — same user_id+platform yields one row (happy)", async () => {
    await call("/auth/tiktok", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        user_id: "u-tt-upsert",
        session_token: "s1",
        stream_key: "first-key",
      }),
    });
    await call("/auth/tiktok", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        user_id: "u-tt-upsert",
        session_token: "s2",
        stream_key: "second-key",
      }),
    });
    const rows = await env.DB.prepare(
      "SELECT stream_key FROM platform_credentials WHERE user_id = ? AND platform = ?",
    )
      .bind("u-tt-upsert", "tiktok")
      .all();
    expect(rows.results).toHaveLength(1);
    expect((rows.results[0] as { stream_key: string }).stream_key).toBe(
      "second-key",
    );
  });
});

describe("POST /api/sessions — Grip paste-creds vs Seller API routing", () => {
  // Task B: verify the orchestration layer (app.ts §~340-460) correctly
  // picks the paste-creds path vs the Seller-API path for grip destinations.
  //
  // The branch selector lives at app.ts:350-365:
  //   platform=="grip" && product_id && GRIP_ACCESS_KEY && GRIP_SECRET_KEY
  //       → Seller API (auto-provision)
  //   platform=="grip" && !product_id
  //       → manual paste-creds (use rtmp_url/stream_key from the request)
  //
  // Test vitest.config.ts seeds GRIP_ACCESS_KEY/SECRET so the Seller-API
  // branch is reachable. Tests that want the manual path simply omit
  // product_id.

  it("without product_id → uses pasted rtmp_url + stream_key and does NOT hit Seller API (happy)", async () => {
    // Seed a user + a saved Grip credential row (the row is not consulted
    // by this code path today, but mirrors real-world state: creator has
    // both pasted AND picked a specific destination).
    await seedUser("u-grip-paste");
    await env.DB.prepare(
      "INSERT INTO voices (id, user_id, elevenlabs_voice_id, name, source_lang, created_at) VALUES (?, ?, ?, ?, ?, ?)",
    )
      .bind(
        "v-grip-paste",
        "u-grip-paste",
        "el-vpaste",
        "Aziz",
        "ko",
        Math.floor(Date.now() / 1000),
      )
      .run();
    await env.DB.prepare(
      "INSERT INTO platform_credentials (id, user_id, platform, rtmp_url, stream_key, display_name, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
      .bind(
        "pc-gp",
        "u-grip-paste",
        "grip",
        "rtmps://legacy.grip/live/",
        "legacy-sk",
        "grip:legacy",
        Math.floor(Date.now() / 1000),
        Math.floor(Date.now() / 1000),
      )
      .run();

    // Installation-level fetch sentry: any network call at all is a bug in
    // the paste-creds path. If the Seller-API branch fires by mistake, it
    // would call fetch("…/broadcasts…") and our stub would throw.
    let fetchCalls = 0;
    const original = globalThis.fetch;
    vi.stubGlobal(
      "fetch",
      vi.fn(async (...args: Parameters<typeof fetch>) => {
        fetchCalls += 1;
        // Delegate to the real fetch so any other unrelated call still works.
        return original(...args);
      }),
    );

    const res = await call("/api/sessions", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        user_id: "u-grip-paste",
        title: "Paste creds",
        source_lang: "ko",
        target_langs: ["zh"],
        // No product_id → orchestration must fall through to manual paste,
        // using the rtmp_url+stream_key supplied RIGHT HERE (not the
        // platform_credentials row — that row is for the /auth/grip flow).
        platforms: [
          {
            platform: "grip",
            lang: "zh",
            rtmp_url: "rtmps://paste.grip/live/",
            stream_key: "paste-sk-xxx",
          },
        ],
      }),
    });
    expect(res.status).toBe(200);
    expect(fetchCalls).toBe(0); // no Seller-API call

    // The inserted streams row must carry the pasted values, NOT any
    // stubbed Seller-API response values.
    const streamRows = await env.DB.prepare(
      "SELECT rtmp_url, stream_key FROM streams WHERE lang = ? AND platform = ?",
    )
      .bind("zh", "grip")
      .all();
    expect(streamRows.results).toHaveLength(1);
    const streamRow = streamRows.results[0] as {
      rtmp_url: string;
      stream_key: string;
    };
    expect(streamRow.rtmp_url).toBe("rtmps://paste.grip/live/");
    expect(streamRow.stream_key).toBe("paste-sk-xxx");
  });

  it("with product_id but Seller API returns 404 → 502 with rolled-back session (sad)", async () => {
    // Task B negative: when product_id triggers the Seller-API branch and
    // the API rejects (test env has no real Grip), the orchestration layer
    // MUST roll back the session + stream rows and surface a non-2xx. The
    // FE then either prompts the user to paste creds or retries. See
    // app.ts:423-436 for the rollback + status-mapping logic.
    await seedUser("u-grip-prod");

    installFetchStub([
      {
        // Match any Grip seller-api endpoint and reply with 404 so the
        // handler goes down the `502` branch (401/403 are passed through;
        // anything else → 502).
        match: /grip/i,
        method: "POST",
        reply: () => new Response("not found", { status: 404 }),
      },
    ]);

    const res = await call("/api/sessions", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        user_id: "u-grip-prod",
        title: "Auto provision",
        source_lang: "en",
        target_langs: ["ja"],
        platforms: [
          {
            platform: "grip",
            lang: "ja",
            product_id: "prod_abc123",
          },
        ],
      }),
    });

    // 404 on the Seller API maps to 502 per app.ts:435. 401/403 pass
    // through unchanged — here we deliberately chose 404 to pin the
    // `502 Bad Gateway` mapping branch.
    expect(res.status).toBe(502);

    // Rollback invariant: no session + no stream rows survive a failed
    // auto-provision. The `insertedIds` cleanup + deleteSessionRow chain
    // is what keeps this promise.
    const sessionRows = await env.DB.prepare(
      "SELECT id FROM sessions WHERE user_id = ?",
    )
      .bind("u-grip-prod")
      .all();
    expect(sessionRows.results).toHaveLength(0);

    const streamRows = await env.DB.prepare("SELECT id FROM streams").all();
    expect(streamRows.results).toHaveLength(0);
  });
});

describe("GET /api/billing/summary", () => {
  it("returns zeros for a user with no sessions (happy)", async () => {
    const res = await call("/api/billing/summary?user_id=u-none");
    expect(res.status).toBe(200);
    const body = (await res.json()) as {
      source_minutes: number;
      output_minutes_by_lang: Record<string, number>;
      estimated_cost_usd: number;
    };
    expect(body.source_minutes).toBe(0);
    expect(body.output_minutes_by_lang).toEqual({});
    expect(body.estimated_cost_usd).toBe(0);
  });

  it("400 when user_id missing (sad)", async () => {
    const res = await call("/api/billing/summary");
    expect(res.status).toBe(400);
  });

  it("aggregates usage across multiple sessions of one user (happy)", async () => {
    await env.DB.prepare(
      "INSERT INTO users (id, created_at) VALUES (?, ?)",
    ).bind("u-agg", Math.floor(Date.now() / 1000)).run();

    const mkSession = async (titles: string) => {
      const res = await call("/api/sessions", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          user_id: "u-agg",
          title: titles,
          source_lang: "en",
          target_langs: ["ja"],
        }),
      });
      const body = (await res.json()) as { session: { id: string } };
      return body.session.id;
    };
    const s1 = await mkSession("one");
    const s2 = await mkSession("two");

    for (const sid of [s1, s2]) {
      await call(`/internal/sessions/${sid}/metrics`, {
        method: "PATCH",
        headers: {
          "Content-Type": "application/json",
          "X-Internal-Secret": env.INTERNAL_SECRET,
        },
        body: JSON.stringify({
          source_seconds: 60,
          output_seconds_by_lang: { ja: 60 },
        }),
      });
    }

    const res = await call("/api/billing/summary?user_id=u-agg");
    const body = (await res.json()) as {
      source_minutes: number;
      output_minutes_by_lang: Record<string, number>;
      estimated_cost_usd: number;
      period_start: number;
      period_end: number;
    };
    expect(body.source_minutes).toBe(2); // 120s total
    expect(body.output_minutes_by_lang.ja).toBe(2); // 120s total
    expect(body.estimated_cost_usd).toBe(3); // 2 × 1.5
    expect(body.period_end).toBeGreaterThan(body.period_start);
  });
});

describe("GET /api/sessions/:id/usage + PATCH /internal/sessions/:id/metrics", () => {
  it("GET returns zeros until metrics are PATCHed (happy)", async () => {
    await env.DB.prepare(
      "INSERT INTO users (id, created_at) VALUES (?, ?)",
    ).bind("u-usage", Math.floor(Date.now() / 1000)).run();
    const create = await call("/api/sessions", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        user_id: "u-usage",
        title: "U",
        source_lang: "en",
        target_langs: ["ja"],
      }),
    });
    const { session } = (await create.json()) as { session: { id: string } };

    const before = await call(`/api/sessions/${session.id}/usage`);
    const beforeBody = (await before.json()) as {
      source_minutes: number;
      output_minutes_by_lang: Record<string, number>;
    };
    expect(beforeBody.source_minutes).toBe(0);
    expect(beforeBody.output_minutes_by_lang).toEqual({});

    const patch = await call(`/internal/sessions/${session.id}/metrics`, {
      method: "PATCH",
      headers: {
        "Content-Type": "application/json",
        "X-Internal-Secret": env.INTERNAL_SECRET,
      },
      body: JSON.stringify({
        source_seconds: 180,
        output_seconds_by_lang: { ja: 120 },
      }),
    });
    expect(patch.status).toBe(200);

    const after = await call(`/api/sessions/${session.id}/usage`);
    const afterBody = (await after.json()) as {
      source_minutes: number;
      output_minutes_by_lang: Record<string, number>;
    };
    expect(afterBody.source_minutes).toBe(3);
    expect(afterBody.output_minutes_by_lang.ja).toBe(2);
  });

  it("PATCH merges per-lang outputs without clobbering prior langs (edge)", async () => {
    await env.DB.prepare(
      "INSERT INTO users (id, created_at) VALUES (?, ?)",
    ).bind("u-merge", Math.floor(Date.now() / 1000)).run();
    const create = await call("/api/sessions", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        user_id: "u-merge",
        title: "M",
        source_lang: "en",
        target_langs: ["ja", "ko"],
      }),
    });
    const { session } = (await create.json()) as { session: { id: string } };
    const patchOnce = async (payload: unknown) =>
      call(`/internal/sessions/${session.id}/metrics`, {
        method: "PATCH",
        headers: {
          "Content-Type": "application/json",
          "X-Internal-Secret": env.INTERNAL_SECRET,
        },
        body: JSON.stringify(payload),
      });

    await patchOnce({ output_seconds_by_lang: { ja: 60 } });
    await patchOnce({ output_seconds_by_lang: { ko: 90 } });

    const res = await call(`/api/sessions/${session.id}/usage`);
    const body = (await res.json()) as {
      output_minutes_by_lang: Record<string, number>;
    };
    expect(body.output_minutes_by_lang.ja).toBe(1);
    expect(body.output_minutes_by_lang.ko).toBe(1.5);
  });

  it("PATCH without X-Internal-Secret → 401 (sad)", async () => {
    const res = await call("/internal/sessions/any/metrics", {
      method: "PATCH",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ source_seconds: 1 }),
    });
    expect(res.status).toBe(401);
  });

  it("PATCH on unknown session → 404 (sad)", async () => {
    const res = await call("/internal/sessions/does-not-exist/metrics", {
      method: "PATCH",
      headers: {
        "Content-Type": "application/json",
        "X-Internal-Secret": env.INTERNAL_SECRET,
      },
      body: JSON.stringify({ source_seconds: 10 }),
    });
    expect(res.status).toBe(404);
  });
});

describe("POST /stripe/webhook", () => {
  async function hmacHex(secret: string, payload: string): Promise<string> {
    const key = await crypto.subtle.importKey(
      "raw",
      new TextEncoder().encode(secret),
      { name: "HMAC", hash: "SHA-256" },
      false,
      ["sign"],
    );
    const sig = await crypto.subtle.sign(
      "HMAC",
      key,
      new TextEncoder().encode(payload),
    );
    let out = "";
    for (const byte of new Uint8Array(sig)) out += byte.toString(16).padStart(2, "0");
    return out;
  }

  it("400 when signature is missing (sad)", async () => {
    const res = await call("/stripe/webhook", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ type: "ping" }),
    });
    expect(res.status).toBe(400);
  });

  it("200 when signature valid (happy)", async () => {
    const t = Math.floor(Date.now() / 1000);
    const payload = JSON.stringify({ type: "invoice.paid" });
    const sig = await hmacHex(env.STRIPE_WEBHOOK_SECRET, `${t}.${payload}`);
    const res = await call("/stripe/webhook", {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        "Stripe-Signature": `t=${t},v1=${sig}`,
      },
      body: payload,
    });
    expect(res.status).toBe(200);
    const body = (await res.json()) as { received: boolean; verified: boolean };
    expect(body.verified).toBe(true);
  });

  it("400 when signature tampered (sad)", async () => {
    const t = Math.floor(Date.now() / 1000);
    const payload = JSON.stringify({ type: "invoice.paid" });
    const res = await call("/stripe/webhook", {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        "Stripe-Signature": `t=${t},v1=${"0".repeat(64)}`,
      },
      body: payload,
    });
    expect(res.status).toBe(400);
  });

  it("200 + verified=false when STRIPE_WEBHOOK_SECRET is unset (edge)", async () => {
    const overrideEnv = { ...env, STRIPE_WEBHOOK_SECRET: undefined };
    const res = await app.fetch(
      new Request("https://test.local/stripe/webhook", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: "{}",
      }),
      overrideEnv,
    );
    expect(res.status).toBe(200);
    const body = (await res.json()) as { received: boolean; verified: boolean };
    expect(body.received).toBe(true);
    expect(body.verified).toBe(false);
  });
});

describe("POST /api/user/complete-onboarding", () => {
  it("400 without user_id (sad)", async () => {
    const res = await call("/api/user/complete-onboarding", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: "{}",
    });
    expect(res.status).toBe(400);
  });

  it("stamps onboarding_completed_at on the user (happy)", async () => {
    const before = await call("/api/user?user_id=u-onb");
    const beforeBody = (await before.json()) as {
      onboarding_completed_at: number | null;
    };
    expect(beforeBody.onboarding_completed_at).toBeNull();

    const res = await call("/api/user/complete-onboarding", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ user_id: "u-onb" }),
    });
    expect(res.status).toBe(200);
    const body = (await res.json()) as {
      onboarding_completed_at: number | null;
    };
    expect(body.onboarding_completed_at).toBeGreaterThan(0);
  });

  it("is idempotent — second call keeps a truthy stamp (happy)", async () => {
    await call("/api/user/complete-onboarding", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ user_id: "u-onb-2" }),
    });
    const second = await call("/api/user/complete-onboarding", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ user_id: "u-onb-2" }),
    });
    const body = (await second.json()) as { onboarding_completed_at: number };
    expect(body.onboarding_completed_at).toBeGreaterThan(0);
  });
});

describe("POST /api/voices upsert behaviour", () => {
  it("deletes prior ElevenLabs voice + row before creating a new one (happy)", async () => {
    const elCalls: Array<{ method: string; url: string }> = [];
    vi.stubGlobal(
      "fetch",
      vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
        const url =
          typeof input === "string"
            ? input
            : input instanceof URL
              ? input.toString()
              : input.url;
        const method = (
          init?.method ?? (input instanceof Request ? input.method : "GET")
        ).toUpperCase();
        elCalls.push({ method, url });
        if (/api\.elevenlabs\.io\/v1\/voices\/add/.test(url)) {
          return new Response(JSON.stringify({ voice_id: `el-${elCalls.length}` }), {
            status: 200,
            headers: { "Content-Type": "application/json" },
          });
        }
        if (/api\.elevenlabs\.io\/v1\/voices\//.test(url) && method === "DELETE") {
          return new Response("", { status: 200 });
        }
        throw new Error(`unstubbed fetch: ${method} ${url}`);
      }),
    );

    const first = await call("/api/voices", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        user_id: "u-upsert",
        name: "v1",
        audio_base64: VALID_WAV_B64,
      }),
    });
    expect(first.status).toBe(200);
    const firstVoice = (await first.json()) as {
      id: string;
      elevenlabs_voice_id: string;
    };

    const second = await call("/api/voices", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        user_id: "u-upsert",
        name: "v2",
        audio_base64: VALID_WAV_B64,
      }),
    });
    expect(second.status).toBe(200);
    const secondVoice = (await second.json()) as {
      id: string;
      elevenlabs_voice_id: string;
    };
    expect(secondVoice.id).not.toBe(firstVoice.id);

    // One ADD per create, plus one DELETE for the prior clone.
    const adds = elCalls.filter((x) => /voices\/add/.test(x.url));
    const deletes = elCalls.filter(
      (x) => x.method === "DELETE" && /voices\//.test(x.url),
    );
    expect(adds).toHaveLength(2);
    expect(deletes).toHaveLength(1);
    expect(deletes[0]!.url).toContain(firstVoice.elevenlabs_voice_id);

    // Only the new voice remains + user's active_voice_id points at it.
    const list = await call("/api/voices?user_id=u-upsert");
    const listBody = (await list.json()) as { voices: Array<{ id: string }> };
    expect(listBody.voices).toHaveLength(1);
    expect(listBody.voices[0].id).toBe(secondVoice.id);

    const user = await call("/api/user?user_id=u-upsert");
    const userBody = (await user.json()) as { active_voice_id: string | null };
    expect(userBody.active_voice_id).toBe(secondVoice.id);
  });
});

describe("POST /api/voices — tolerates ElevenLabs delete failure on prior voice", () => {
  it("logs + continues when the pre-upsert DELETE throws (edge)", async () => {
    let addCount = 0;
    vi.stubGlobal(
      "fetch",
      vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
        const url =
          typeof input === "string"
            ? input
            : input instanceof URL
              ? input.toString()
              : input.url;
        const method = (
          init?.method ?? (input instanceof Request ? input.method : "GET")
        ).toUpperCase();
        if (/voices\/add/.test(url)) {
          addCount++;
          return new Response(JSON.stringify({ voice_id: `el-v${addCount}` }), {
            status: 200,
          });
        }
        if (method === "DELETE") {
          throw new Error("simulated network failure");
        }
        throw new Error(`unstubbed: ${method} ${url}`);
      }),
    );

    // First upsert establishes an active voice.
    const first = await call("/api/voices", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        user_id: "u-delfail",
        name: "One",
        audio_base64: VALID_WAV_B64,
      }),
    });
    expect(first.status).toBe(200);

    // Second upsert: DELETE throws, but new clone should still land.
    const second = await call("/api/voices", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        user_id: "u-delfail",
        name: "Two",
        audio_base64: VALID_WAV_B64,
      }),
    });
    expect(second.status).toBe(200);
    const body = (await second.json()) as { elevenlabs_voice_id: string };
    expect(body.elevenlabs_voice_id).toBe("el-v2");
  });
});

describe("GET /api/billing/rate", () => {
  it("returns the published self-serve rate (happy)", async () => {
    const res = await call("/api/billing/rate");
    expect(res.status).toBe(200);
    const body = (await res.json()) as { per_output_minute_usd: number };
    expect(body.per_output_minute_usd).toBe(1.5);
  });
});

describe("GET /api/sessions/:id/quote", () => {
  it("projects cost across all target langs (happy)", async () => {
    const createRes = await call("/api/sessions", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        user_id: "u-quote",
        title: "Quote",
        source_lang: "ko",
        target_langs: ["en", "ja", "zh"],
      }),
    });
    const { session } = (await createRes.json()) as { session: { id: string } };

    const res = await call(`/api/sessions/${session.id}/quote?expected_minutes=20`);
    expect(res.status).toBe(200);
    const body = (await res.json()) as {
      output_minutes: number;
      estimated_cost_usd: number;
      per_output_minute_usd: number;
    };
    expect(body.output_minutes).toBe(60); // 20 minutes × 3 targets
    expect(body.estimated_cost_usd).toBe(90); // 60 × 1.5
    expect(body.per_output_minute_usd).toBe(1.5);
  });

  it("user with an active voice clone receives a finite quote (happy)", async () => {
    // User onboarded + has cloned their voice. Rate is voice-agnostic, but
    // this guards against a regression where the handler branches on voice.
    await env.DB.prepare(
      "INSERT INTO users (id, created_at) VALUES (?, ?)",
    ).bind("u-voiced", Math.floor(Date.now() / 1000)).run();
    // Seed a real voice row + attach it so the FK holds. source_lang matches
    // the session's source_lang below so the mismatch guard stays silent.
    await env.DB.prepare(
      "INSERT INTO voices (id, user_id, elevenlabs_voice_id, name, source_lang, created_at) VALUES (?, ?, ?, ?, ?, ?)",
    )
      .bind("v-voiced-1", "u-voiced", "el-voiced-1", "Aziz", "ko", Math.floor(Date.now() / 1000))
      .run();
    await env.DB.prepare("UPDATE users SET active_voice_id = ? WHERE id = ?")
      .bind("v-voiced-1", "u-voiced")
      .run();

    const createRes = await call("/api/sessions", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        user_id: "u-voiced",
        title: "With voice",
        source_lang: "ko",
        target_langs: ["ja"],
      }),
    });
    const { session } = (await createRes.json()) as { session: { id: string } };

    const res = await call(`/api/sessions/${session.id}/quote?expected_minutes=30`);
    expect(res.status).toBe(200);
    const body = (await res.json()) as { estimated_cost_usd: number };
    expect(Number.isFinite(body.estimated_cost_usd)).toBe(true);
    expect(body.estimated_cost_usd).toBe(45); // 30 × 1 × 1.5
  });

  it("fresh user without an active voice still gets a finite quote (no null)", async () => {
    // Reproduces the production bug: a user with no active_voice_id opens
    // the quote modal. Response must be a real number, not null/undefined.
    await env.DB.prepare(
      "INSERT INTO users (id, created_at) VALUES (?, ?)",
    ).bind("u-fresh", Math.floor(Date.now() / 1000)).run();
    const createRes = await call("/api/sessions", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        user_id: "u-fresh",
        title: "No voice yet",
        source_lang: "ko",
        target_langs: ["en", "ja"],
      }),
    });
    const { session } = (await createRes.json()) as { session: { id: string } };

    const res = await call(`/api/sessions/${session.id}/quote?expected_minutes=30`);
    expect(res.status).toBe(200);
    const body = (await res.json()) as { estimated_cost_usd: number | null };
    expect(body.estimated_cost_usd).not.toBeNull();
    expect(typeof body.estimated_cost_usd).toBe("number");
    expect(Number.isFinite(body.estimated_cost_usd as number)).toBe(true);
    expect(body.estimated_cost_usd).toBe(90); // 30 × 2 × 1.5
  });

  it("10-minute slider floor is a finite quote (edge)", async () => {
    const createRes = await call("/api/sessions", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        user_id: "u-10",
        title: "Ten",
        source_lang: "en",
        target_langs: ["ja"],
      }),
    });
    const { session } = (await createRes.json()) as { session: { id: string } };

    const res = await call(`/api/sessions/${session.id}/quote?expected_minutes=10`);
    expect(res.status).toBe(200);
    const body = (await res.json()) as {
      output_minutes: number;
      estimated_cost_usd: number;
    };
    expect(body.output_minutes).toBe(10);
    expect(body.estimated_cost_usd).toBe(15); // 10 × 1 × 1.5
  });

  it("180-minute slider ceiling is a finite quote (edge)", async () => {
    const createRes = await call("/api/sessions", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        user_id: "u-180",
        title: "Big",
        source_lang: "en",
        target_langs: ["ja", "zh"],
      }),
    });
    const { session } = (await createRes.json()) as { session: { id: string } };

    const res = await call(`/api/sessions/${session.id}/quote?expected_minutes=180`);
    expect(res.status).toBe(200);
    const body = (await res.json()) as {
      output_minutes: number;
      estimated_cost_usd: number;
    };
    expect(body.output_minutes).toBe(360); // 180 × 2
    expect(body.estimated_cost_usd).toBe(540); // 360 × 1.5
  });

  it("400 when expected_minutes missing or non-positive (sad)", async () => {
    const createRes = await call("/api/sessions", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        user_id: "u-q2",
        title: "Q",
        source_lang: "en",
        target_langs: ["ja"],
      }),
    });
    const { session } = (await createRes.json()) as { session: { id: string } };

    const missing = await call(`/api/sessions/${session.id}/quote`);
    expect(missing.status).toBe(400);

    const zero = await call(`/api/sessions/${session.id}/quote?expected_minutes=0`);
    expect(zero.status).toBe(400);
  });

  it("404 when session does not exist (sad)", async () => {
    const res = await call("/api/sessions/nope/quote?expected_minutes=10");
    expect(res.status).toBe(404);
  });

  it("returns 0 output minutes when target_langs is corrupted JSON (edge)", async () => {
    await env.DB.prepare(
      "INSERT INTO users (id, created_at) VALUES (?, ?)",
    ).bind("u-bad", Math.floor(Date.now() / 1000)).run();
    const createRes = await call("/api/sessions", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        user_id: "u-bad",
        title: "bad",
        source_lang: "en",
        target_langs: ["ja"],
      }),
    });
    const { session } = (await createRes.json()) as { session: { id: string } };
    // Corrupt target_langs to exercise the JSON.parse catch branch.
    await env.DB.prepare(
      "UPDATE sessions SET target_langs = ? WHERE id = ?",
    ).bind("not json at all", session.id).run();

    const res = await call(`/api/sessions/${session.id}/quote?expected_minutes=10`);
    const body = (await res.json()) as { output_minutes: number; estimated_cost_usd: number };
    expect(body.output_minutes).toBe(0); // 10 × 0-langs
    expect(body.estimated_cost_usd).toBe(0);
  });
});

describe("GET /api/sessions/:id/summary", () => {
  it("is_final=false while session is in setup/live (happy)", async () => {
    const createRes = await call("/api/sessions", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        user_id: "u-sum",
        title: "S",
        source_lang: "en",
        target_langs: ["ja"],
      }),
    });
    const { session } = (await createRes.json()) as { session: { id: string } };

    const res = await call(`/api/sessions/${session.id}/summary`);
    expect(res.status).toBe(200);
    const body = (await res.json()) as {
      status: string;
      is_final: boolean;
      source_minutes: number;
      estimated_cost_usd: number;
    };
    expect(body.status).toBe("setup");
    expect(body.is_final).toBe(false);
    expect(body.source_minutes).toBe(0);
    expect(body.estimated_cost_usd).toBe(0);
  });

  it("is_final=true once status leaves setup/live; uses 1.5 rate (happy)", async () => {
    const createRes = await call("/api/sessions", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        user_id: "u-sum-2",
        title: "S",
        source_lang: "en",
        target_langs: ["ja"],
      }),
    });
    const { session } = (await createRes.json()) as { session: { id: string } };

    await call(`/internal/sessions/${session.id}/metrics`, {
      method: "PATCH",
      headers: {
        "Content-Type": "application/json",
        "X-Internal-Secret": env.INTERNAL_SECRET,
      },
      body: JSON.stringify({
        source_seconds: 120,
        output_seconds_by_lang: { ja: 120 },
      }),
    });
    await call(`/internal/sessions/${session.id}`, {
      method: "PATCH",
      headers: {
        "Content-Type": "application/json",
        "X-Internal-Secret": env.INTERNAL_SECRET,
      },
      body: JSON.stringify({ status: "ended" }),
    });

    const res = await call(`/api/sessions/${session.id}/summary`);
    const body = (await res.json()) as {
      status: string;
      is_final: boolean;
      source_minutes: number;
      output_minutes_by_lang: Record<string, number>;
      estimated_cost_usd: number;
    };
    expect(body.status).toBe("ended");
    expect(body.is_final).toBe(true);
    expect(body.source_minutes).toBe(2); // 120s → 2min
    expect(body.output_minutes_by_lang.ja).toBe(2);
    expect(body.estimated_cost_usd).toBe(3); // 2 × 1.5
  });

  it("404 when session does not exist (sad)", async () => {
    const res = await call("/api/sessions/nope/summary");
    expect(res.status).toBe(404);
  });
});

