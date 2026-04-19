import { describe, it, expect } from "vitest";

import { verifyStripeSignature } from "../src/features/billing/stripe-webhook";

const SECRET = "whsec_test_secret";

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

describe("verifyStripeSignature", () => {
  it("accepts a valid signature + surfaces event type (happy)", async () => {
    const t = Math.floor(Date.now() / 1000);
    const payload = JSON.stringify({ type: "invoice.paid" });
    const sig = await hmacHex(SECRET, `${t}.${payload}`);
    const result = await verifyStripeSignature(payload, `t=${t},v1=${sig}`, SECRET, t);
    expect(result.ok).toBe(true);
    if (result.ok) expect(result.eventType).toBe("invoice.paid");
  });

  it("returns ok=true eventType=null when payload isn't valid JSON (edge)", async () => {
    const t = Math.floor(Date.now() / 1000);
    const payload = "not-json";
    const sig = await hmacHex(SECRET, `${t}.${payload}`);
    const result = await verifyStripeSignature(payload, `t=${t},v1=${sig}`, SECRET, t);
    expect(result.ok).toBe(true);
    if (result.ok) expect(result.eventType).toBeNull();
  });

  it("rejects when signature header is missing (sad)", async () => {
    const r = await verifyStripeSignature("{}", null, SECRET, 1);
    expect(r.ok).toBe(false);
    if (!r.ok) expect(r.reason).toMatch(/missing/);
  });

  it("rejects when signature header is malformed (sad)", async () => {
    const r = await verifyStripeSignature("{}", "not-a-stripe-sig", SECRET, 1);
    expect(r.ok).toBe(false);
    if (!r.ok) expect(r.reason).toMatch(/malformed/);
  });

  it("rejects when the signed timestamp is outside tolerance (sad)", async () => {
    const t = 1000;
    const payload = "{}";
    const sig = await hmacHex(SECRET, `${t}.${payload}`);
    const r = await verifyStripeSignature(
      payload,
      `t=${t},v1=${sig}`,
      SECRET,
      t + 1000, // 1000s skew >> 5min tolerance
    );
    expect(r.ok).toBe(false);
    if (!r.ok) expect(r.reason).toMatch(/tolerance/);
  });

  it("rejects when the signature hex is the wrong length (sad)", async () => {
    const t = Math.floor(Date.now() / 1000);
    const r = await verifyStripeSignature(
      "{}",
      `t=${t},v1=deadbeef`, // expected sig is 64 hex chars, this is 8
      SECRET,
      t,
    );
    expect(r.ok).toBe(false);
    if (!r.ok) expect(r.reason).toMatch(/mismatch/);
  });

  it("ignores parts with no `=` and unknown keys (edge)", async () => {
    // no-pair `bogus` + unknown-key `other=foo` + valid t/v1 pair
    const t = Math.floor(Date.now() / 1000);
    const payload = "{}";
    const sig = await hmacHex(SECRET, `${t}.${payload}`);
    const r = await verifyStripeSignature(
      payload,
      `bogus,other=foo,t=${t},v1=${sig}`,
      SECRET,
      t,
    );
    expect(r.ok).toBe(true);
    if (r.ok) expect(r.eventType).toBeNull();
  });

  it("parses signed payload where `type` is missing → eventType=null (edge)", async () => {
    const t = Math.floor(Date.now() / 1000);
    const payload = JSON.stringify({ other: "data" }); // no `type` key
    const sig = await hmacHex(SECRET, `${t}.${payload}`);
    const r = await verifyStripeSignature(payload, `t=${t},v1=${sig}`, SECRET, t);
    expect(r.ok).toBe(true);
    if (r.ok) expect(r.eventType).toBeNull();
  });

  it("accepts when any listed v1 signature matches (edge)", async () => {
    const t = Math.floor(Date.now() / 1000);
    const payload = JSON.stringify({ type: "ping" });
    const sig = await hmacHex(SECRET, `${t}.${payload}`);
    const r = await verifyStripeSignature(
      payload,
      `t=${t},v1=${"0".repeat(64)},v1=${sig}`,
      SECRET,
      t,
    );
    expect(r.ok).toBe(true);
  });
});
