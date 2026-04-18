import { describe, it, expect } from "vitest";
import { SignJWT } from "jose";

import { signJwt, verifyJwt } from "../src/auth";

const SECRET = "unit-test-hs256-secret-at-least-32-chars";

describe("signJwt → verifyJwt roundtrip (happy paths)", () => {
  it("returns the user id on the `sub` claim", async () => {
    const token = await signJwt(SECRET, { sub: "user-1" });
    const claims = await verifyJwt(SECRET, token);
    expect(claims.sub).toBe("user-1");
  });

  it("carries optional voice_clone_id + session_id claims through", async () => {
    const token = await signJwt(SECRET, {
      sub: "user-1",
      voice_clone_id: "el-voice-abc",
      session_id: "sess-xyz",
    });
    const claims = await verifyJwt(SECRET, token);
    expect(claims.voice_clone_id).toBe("el-voice-abc");
    expect(claims.session_id).toBe("sess-xyz");
  });
});

describe("verifyJwt sad paths", () => {
  it("rejects a token signed with a different secret", async () => {
    const token = await signJwt(SECRET, { sub: "user-1" });
    await expect(verifyJwt("different-secret-0000000000000000", token)).rejects.toThrow();
  });

  it("rejects a garbled / non-JWT string", async () => {
    await expect(verifyJwt(SECRET, "not.a.jwt")).rejects.toThrow();
    await expect(verifyJwt(SECRET, "")).rejects.toThrow();
  });

  it("rejects a token whose audience does not match", async () => {
    const key = new TextEncoder().encode(SECRET);
    const token = await new SignJWT({ sub: "user-1" })
      .setProtectedHeader({ alg: "HS256", typ: "JWT" })
      .setIssuer("brivva-api")
      .setAudience("some-other-service") // not brivva-fargate
      .setSubject("user-1")
      .setExpirationTime("15m")
      .sign(key);

    await expect(verifyJwt(SECRET, token)).rejects.toThrow();
  });

  it("rejects a token from an unrecognized issuer", async () => {
    const key = new TextEncoder().encode(SECRET);
    const token = await new SignJWT({ sub: "user-1" })
      .setProtectedHeader({ alg: "HS256", typ: "JWT" })
      .setIssuer("evil-issuer")
      .setAudience("brivva-fargate")
      .setSubject("user-1")
      .setExpirationTime("15m")
      .sign(key);

    await expect(verifyJwt(SECRET, token)).rejects.toThrow();
  });

  it("rejects a token whose expiration is in the past", async () => {
    const key = new TextEncoder().encode(SECRET);
    const token = await new SignJWT({ sub: "user-1" })
      .setProtectedHeader({ alg: "HS256", typ: "JWT" })
      .setIssuer("brivva-api")
      .setAudience("brivva-fargate")
      .setSubject("user-1")
      .setIssuedAt(Math.floor(Date.now() / 1000) - 7200) // issued 2h ago
      .setExpirationTime(Math.floor(Date.now() / 1000) - 3600) // expired 1h ago
      .sign(key);

    await expect(verifyJwt(SECRET, token)).rejects.toThrow();
  });
});
