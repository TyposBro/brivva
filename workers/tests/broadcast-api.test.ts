// Unit tests for the YouTube liveBroadcasts/liveStreams/bind wrapper.
// The OAuth client (google-oauth-client.ts) has its own tests; here we only
// exercise the 3-call choreography + error + refresh paths.

import { describe, it, expect, vi, afterEach } from "vitest";

import {
  completeYouTubeBroadcast,
  createYouTubeBroadcast,
  YouTubeBroadcastError,
} from "../src/features/youtube/broadcast-api";
import type { Env } from "../src/core/types";

const ENV = {
  GOOGLE_CLIENT_ID: "cid",
  GOOGLE_CLIENT_SECRET: "csec",
  OAUTH_REDIRECT_URI: "https://api.example.com/auth/youtube/callback",
} as unknown as Env;

afterEach(() => vi.unstubAllGlobals());

type FetchFn = (
  input: RequestInfo | URL,
  init?: RequestInit,
) => Promise<Response>;

/** Install a fetch stub that matches each call by URL substring + method. */
function stubYouTubeCalls(
  queue: Array<{ match: RegExp; method: string; reply: Response }>,
): ReturnType<typeof vi.fn> {
  const calls: Array<{ url: string; method: string; body?: unknown }> = [];
  let idx = 0;
  const fn = vi.fn<FetchFn>(async (input, init) => {
    const url = typeof input === "string" ? input : input.toString();
    const method = (init?.method ?? "GET").toUpperCase();
    const rawBody = init?.body;
    let body: unknown = undefined;
    if (typeof rawBody === "string") {
      try {
        body = JSON.parse(rawBody);
      } catch {
        body = rawBody;
      }
    }
    calls.push({ url, method, body });
    const next = queue[idx++];
    if (!next) throw new Error(`unexpected extra fetch: ${method} ${url}`);
    if (!next.match.test(url)) {
      throw new Error(
        `expected ${next.match} got ${url} (call #${idx})`,
      );
    }
    if (next.method.toUpperCase() !== method) {
      throw new Error(
        `expected ${next.method} got ${method} (call #${idx})`,
      );
    }
    return next.reply.clone();
  });
  vi.stubGlobal("fetch", fn);
  (fn as unknown as { calls: typeof calls }).calls = calls;
  return fn;
}

function jsonResponse(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "Content-Type": "application/json" },
  });
}

describe("createYouTubeBroadcast — happy path", () => {
  it("calls insert-broadcast, insert-stream, bind in order and returns the watch URL", async () => {
    stubYouTubeCalls([
      {
        match: /liveBroadcasts\?/,
        method: "POST",
        reply: jsonResponse({
          id: "bcast-123",
          snippet: { title: "My Live" },
          status: { lifeCycleStatus: "created" },
        }),
      },
      {
        match: /liveStreams\?/,
        method: "POST",
        reply: jsonResponse({
          id: "stream-456",
          cdn: {
            ingestionInfo: {
              ingestionAddress: "rtmp://a.rtmp.youtube.com/live2",
              rtmpsIngestionAddress: "rtmps://a.rtmps.youtube.com/live2",
              streamName: "secret-key-abc",
            },
          },
        }),
      },
      {
        match: /liveBroadcasts\/bind/,
        method: "POST",
        reply: jsonResponse({ id: "bcast-123" }),
      },
    ]);

    const result = await createYouTubeBroadcast(ENV, {
      accessToken: "tok",
      title: "My Live",
      scheduledStartTime: "2026-04-19T12:00:00Z",
      privacyStatus: "unlisted",
    });

    expect(result.broadcastId).toBe("bcast-123");
    expect(result.streamId).toBe("stream-456");
    expect(result.rtmpUrl).toBe("rtmps://a.rtmps.youtube.com/live2");
    expect(result.streamKey).toBe("secret-key-abc");
    expect(result.watchUrl).toBe("https://www.youtube.com/watch?v=bcast-123");
    const calls = (fetch as unknown as { calls: Array<{ body?: unknown }> }).calls;
    expect(calls[0].body).toMatchObject({
      contentDetails: { enableAutoStart: true, enableAutoStop: false },
    });
  });
});

describe("createYouTubeBroadcast — token refresh", () => {
  it("refreshes on 401 + retries the broadcast insert (happy)", async () => {
    const onTokenRefresh = vi.fn(async () => {});
    stubYouTubeCalls([
      // First try → 401.
      {
        match: /liveBroadcasts\?/,
        method: "POST",
        reply: new Response("auth", { status: 401 }),
      },
      // OAuth refresh.
      {
        match: /oauth2\.googleapis\.com\/token/,
        method: "POST",
        reply: jsonResponse({ access_token: "new-tok", expires_in: 3600 }),
      },
      // Retry → 200.
      {
        match: /liveBroadcasts\?/,
        method: "POST",
        reply: jsonResponse({ id: "bcast-xyz" }),
      },
      {
        match: /liveStreams\?/,
        method: "POST",
        reply: jsonResponse({
          id: "stream-1",
          cdn: {
            ingestionInfo: {
              ingestionAddress: "rtmp://a.rtmp.youtube.com/live2",
              rtmpsIngestionAddress: "rtmps://a.rtmps.youtube.com/live2",
              streamName: "k",
            },
          },
        }),
      },
      {
        match: /liveBroadcasts\/bind/,
        method: "POST",
        reply: jsonResponse({ id: "bcast-xyz" }),
      },
    ]);

    const result = await createYouTubeBroadcast(
      ENV,
      {
        accessToken: "expired-tok",
        title: "X",
        scheduledStartTime: "2026-04-19T12:00:00Z",
        privacyStatus: "unlisted",
      },
      {
        refreshToken: "rtok",
        onTokenRefresh,
      },
    );

    expect(result.broadcastId).toBe("bcast-xyz");
    expect(onTokenRefresh).toHaveBeenCalledWith(
      "new-tok",
      expect.any(Number),
    );
  });

  it("rejects with YouTubeBroadcastError when no refresh token + 401 (sad)", async () => {
    stubYouTubeCalls([
      {
        match: /liveBroadcasts\?/,
        method: "POST",
        reply: new Response("auth", { status: 401 }),
      },
    ]);

    await expect(
      createYouTubeBroadcast(ENV, {
        accessToken: "x",
        title: "T",
        scheduledStartTime: "2026-04-19T12:00:00Z",
        privacyStatus: "unlisted",
      }),
    ).rejects.toBeInstanceOf(YouTubeBroadcastError);
  });
});

describe("completeYouTubeBroadcast", () => {
  it("transitions a live broadcast to complete", async () => {
    const fetchStub = stubYouTubeCalls([
      {
        match: /liveBroadcasts\/transition\?broadcastStatus=complete&id=bcast-123&part=status/,
        method: "POST",
        reply: jsonResponse({ id: "bcast-123", status: { lifeCycleStatus: "complete" } }),
      },
    ]);

    await completeYouTubeBroadcast(ENV, {
      accessToken: "tok",
      broadcastId: "bcast-123",
    });

    expect(fetchStub).toHaveBeenCalledTimes(1);
  });
});

describe("createYouTubeBroadcast — failure modes", () => {
  it("propagates 403 as YouTubeBroadcastError with status=403 (sad)", async () => {
    stubYouTubeCalls([
      {
        match: /liveBroadcasts\?/,
        method: "POST",
        reply: new Response("quotaExceeded", { status: 403 }),
      },
    ]);

    const e = await createYouTubeBroadcast(ENV, {
      accessToken: "x",
      title: "T",
      scheduledStartTime: "2026-04-19T12:00:00Z",
      privacyStatus: "unlisted",
    }).catch((err) => err);

    expect(e).toBeInstanceOf(YouTubeBroadcastError);
    expect((e as YouTubeBroadcastError).status).toBe(403);
    expect((e as Error).message).toContain("quotaExceeded");
  });

  it("rejects when liveStreams returns no ingestion info (edge)", async () => {
    stubYouTubeCalls([
      {
        match: /liveBroadcasts\?/,
        method: "POST",
        reply: jsonResponse({ id: "b" }),
      },
      {
        match: /liveStreams\?/,
        method: "POST",
        // cdn block is missing — YouTube sometimes returns this for malformed
        // requests but with a 200 status. We treat it as a hard failure.
        reply: jsonResponse({ id: "s" }),
      },
    ]);

    await expect(
      createYouTubeBroadcast(ENV, {
        accessToken: "x",
        title: "T",
        scheduledStartTime: "2026-04-19T12:00:00Z",
        privacyStatus: "unlisted",
      }),
    ).rejects.toThrow(/no ingestion info/);
  });
});
