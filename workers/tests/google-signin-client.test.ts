import { describe, it, expect, vi, afterEach } from "vitest";

import {
  authorizeUrl,
  exchangeCode,
  fetchUserInfo,
} from "../src/features/auth/google-signin-client";

const ENV = {
  GOOGLE_CLIENT_ID: "cid",
  GOOGLE_CLIENT_SECRET: "csecret",
  GOOGLE_SIGNIN_REDIRECT_URI: "https://api.example.com/auth/google/callback",
};

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("authorizeUrl", () => {
  it("embeds openid+email+profile scope + prompt=select_account (happy)", () => {
    const url = authorizeUrl(ENV, "csrf-1");
    expect(url).toContain("scope=openid+email+profile");
    expect(url).toContain("prompt=select_account");
    expect(url).toContain("state=csrf-1");
    expect(url).toContain(encodeURIComponent(ENV.GOOGLE_SIGNIN_REDIRECT_URI));
    expect(url).toContain("access_type=online");
  });
});

describe("exchangeCode", () => {
  it("POSTs form-urlencoded to Google and returns tokens (happy)", async () => {
    let body: string | null = null;
    vi.stubGlobal(
      "fetch",
      vi.fn(async (_url: RequestInfo | URL, init?: RequestInit) => {
        body = (init?.body as URLSearchParams | null)?.toString() ?? null;
        return new Response(
          JSON.stringify({
            access_token: "A",
            id_token: "id",
            expires_in: 3600,
            token_type: "Bearer",
            scope: "openid email profile",
          }),
          { status: 200, headers: { "Content-Type": "application/json" } },
        );
      }),
    );
    const r = await exchangeCode(ENV, "code-abc");
    expect(r.access_token).toBe("A");
    expect(body).toContain("grant_type=authorization_code");
    expect(body).toContain("code=code-abc");
  });

  it("throws when Google returns non-200 (sad)", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => new Response("bad grant", { status: 400 })),
    );
    await expect(exchangeCode(ENV, "x")).rejects.toThrow(/OAuth token exchange failed/);
  });
});

describe("fetchUserInfo", () => {
  it("maps userinfo fields and never returns undefined (happy)", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () =>
        new Response(
          JSON.stringify({
            sub: "google-1",
            email: "u@ex.com",
            name: "U",
            picture: "pic",
          }),
          { status: 200, headers: { "Content-Type": "application/json" } },
        ),
      ),
    );
    const info = await fetchUserInfo("access");
    expect(info).toEqual({
      sub: "google-1",
      email: "u@ex.com",
      name: "U",
      picture: "pic",
    });
  });

  it("defaults optional fields to null when Google omits them (edge)", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () =>
        new Response(JSON.stringify({ sub: "google-2" }), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        }),
      ),
    );
    const info = await fetchUserInfo("access");
    expect(info.email).toBeNull();
    expect(info.name).toBeNull();
    expect(info.picture).toBeNull();
  });

  it("throws when userinfo responds non-200 (sad)", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => new Response("nope", { status: 401 })),
    );
    await expect(fetchUserInfo("access")).rejects.toThrow(/userinfo failed/);
  });

  it("throws when response omits `sub` (sad)", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () =>
        new Response(JSON.stringify({ email: "no-sub@x" }), { status: 200 }),
      ),
    );
    await expect(fetchUserInfo("access")).rejects.toThrow(/missing sub/);
  });
});
