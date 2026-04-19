import { describe, it, expect, vi, afterEach } from "vitest";

import {
  authorizeUrl,
  exchangeCode,
  getChannelInfo,
  refreshAccessToken,
} from "../src/features/youtube/google-oauth-client";
import type { Env } from "../src/core/types";

const ENV = {
  GOOGLE_CLIENT_ID: "cid",
  GOOGLE_CLIENT_SECRET: "csec",
  OAUTH_REDIRECT_URI: "https://api.example.com/auth/youtube/callback",
} as unknown as Env;

afterEach(() => vi.unstubAllGlobals());

describe("YouTube OAuth client", () => {
  it("authorizeUrl puts YouTube scope + access_type=offline (happy)", () => {
    const url = authorizeUrl(ENV, "user-42");
    expect(url).toContain(encodeURIComponent("youtube.readonly"));
    expect(url).toContain("access_type=offline");
    expect(url).toContain("state=user-42");
  });

  it("exchangeCode returns tokens on 200 (happy)", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () =>
        new Response(
          JSON.stringify({
            access_token: "A",
            refresh_token: "R",
            expires_in: 60,
            token_type: "Bearer",
            scope: "y",
          }),
          { status: 200 },
        ),
      ),
    );
    const r = await exchangeCode(ENV, "code");
    expect(r.access_token).toBe("A");
    expect(r.refresh_token).toBe("R");
  });

  it("exchangeCode throws on non-200 (sad)", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => new Response("err", { status: 400 })),
    );
    await expect(exchangeCode(ENV, "code")).rejects.toThrow(/OAuth token exchange/);
  });

  it("refreshAccessToken returns the new access token (happy)", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () =>
        new Response(
          JSON.stringify({ access_token: "A2", expires_in: 3600 }),
          { status: 200 },
        ),
      ),
    );
    const r = await refreshAccessToken(ENV, "rtok");
    expect(r.access_token).toBe("A2");
    expect(r.expires_in).toBe(3600);
  });

  it("refreshAccessToken throws on non-200 (sad)", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => new Response("err", { status: 401 })),
    );
    await expect(refreshAccessToken(ENV, "rtok")).rejects.toThrow(/OAuth refresh/);
  });

  it("getChannelInfo picks the first channel (happy)", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () =>
        new Response(
          JSON.stringify({
            items: [{ id: "UC1", snippet: { title: "My Channel" } }],
          }),
          { status: 200 },
        ),
      ),
    );
    const r = await getChannelInfo("A");
    expect(r).toEqual({ id: "UC1", title: "My Channel" });
  });

  it("getChannelInfo throws when YouTube returns no channel (sad)", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () =>
        new Response(JSON.stringify({ items: [] }), { status: 200 }),
      ),
    );
    await expect(getChannelInfo("A")).rejects.toThrow(/No channel/);
  });

  it("getChannelInfo throws on non-200 (sad)", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => new Response("denied", { status: 403 })),
    );
    await expect(getChannelInfo("A")).rejects.toThrow(/Channel lookup/);
  });
});
