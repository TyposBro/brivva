// §0.5.1 — real-response schema roundtrip.
//
// For every external vendor (ElevenLabs, Stripe, YouTube, Google OAuth,
// Grip), load the captured fixture and drive it through the real client
// deserializer. If a vendor changes a field name or field type overnight
// (Soniox integer `error_code` April 2026), this file is where it
// breaks first — not production.
//
// Fixtures live under tests/fixtures/<vendor>/. Whether each is CAPTURED
// (from a live API response) or HAND_CRAFTED_PENDING_REAL_CAPTURE (for
// stubs + vendors blocked on support) is documented in the per-vendor
// README. Replace hand-crafted fixtures with real captures as vendor
// access unblocks — the tests in this file must keep passing.

import { describe, it, expect, vi, afterEach } from "vitest";

import voiceCloneHappy from "./fixtures/elevenlabs/voice_clone_happy.json";
import voiceClone402 from "./fixtures/elevenlabs/voice_clone_402_quota.json";
import voiceDeleteHappy from "./fixtures/elevenlabs/voice_delete_happy.json";
import checkoutSessionCompleted from "./fixtures/stripe/checkout_session_completed.json";
import invoicePaymentFailed from "./fixtures/stripe/invoice_payment_failed.json";
import broadcastInsertHappy from "./fixtures/youtube/broadcast_insert_happy.json";
import streamInsertHappy from "./fixtures/youtube/stream_insert_happy.json";
import broadcast403 from "./fixtures/youtube/broadcast_403_quota.json";
import signinTokenExchange from "./fixtures/google-oauth/signin_token_exchange.json";
import signinUserinfo from "./fixtures/google-oauth/signin_userinfo.json";
import signinUserinfoMissingPic from "./fixtures/google-oauth/signin_userinfo_missing_picture.json";
import youtubeTokenExchange from "./fixtures/google-oauth/youtube_token_exchange.json";
import invalidGrant from "./fixtures/google-oauth/invalid_grant.json";
import gripProvisionHappy from "./fixtures/grip/provision_stream_happy.json";
import gripProvision401 from "./fixtures/grip/provision_stream_401_invalid_auth.json";

import {
  cloneVoice,
  deleteRemoteVoice,
} from "../src/features/voices/elevenlabs-client";
import {
  createYouTubeBroadcast,
  YouTubeBroadcastError,
} from "../src/features/youtube/broadcast-api";
import {
  exchangeCode as exchangeSignin,
  fetchUserInfo,
} from "../src/features/auth/google-signin-client";
import {
  exchangeCode as exchangeYouTube,
  refreshAccessToken,
} from "../src/features/youtube/google-oauth-client";
import {
  provisionBroadcast,
  GripSellerApiError,
} from "../src/features/grip/seller-api";
import { verifyStripeSignature } from "../src/features/billing/stripe-webhook";
import type { Env } from "../src/core/types";

afterEach(() => vi.unstubAllGlobals());

type FetchFn = (
  input: RequestInfo | URL,
  init?: RequestInit,
) => Promise<Response> | Response;

function stubFetch(fn: FetchFn) {
  vi.stubGlobal("fetch", vi.fn(fn));
}

const AUDIO_B64 = btoa("\0\0\0\0\0\0\0\0");

const signinEnv = {
  GOOGLE_CLIENT_ID: "id",
  GOOGLE_CLIENT_SECRET: "sec",
  GOOGLE_SIGNIN_REDIRECT_URI: "r",
};

const youtubeEnv = {
  GOOGLE_CLIENT_ID: "id",
  GOOGLE_CLIENT_SECRET: "sec",
  OAUTH_REDIRECT_URI: "r",
} as unknown as Env;

async function hmacSha256Hex(secret: string, payload: string): Promise<string> {
  const key = await crypto.subtle.importKey(
    "raw",
    new TextEncoder().encode(secret),
    { name: "HMAC", hash: "SHA-256" },
    false,
    ["sign"],
  );
  const bytes = new Uint8Array(
    await crypto.subtle.sign("HMAC", key, new TextEncoder().encode(payload)),
  );
  let hex = "";
  for (let i = 0; i < bytes.length; i++) hex += bytes[i]!.toString(16).padStart(2, "0");
  return hex;
}

describe("§0.5.1 fixture roundtrip — ElevenLabs", () => {
  it("voice_clone_happy: cloneVoice extracts a real-shape 20-char voice_id", async () => {
    stubFetch(
      async () => new Response(JSON.stringify(voiceCloneHappy), { status: 200 }),
    );
    const r = await cloneVoice("k", {
      name: "Host",
      audioBase64: AUDIO_B64,
      sourceLang: null,
    });
    expect(r.voice_id).toMatch(/^[A-Za-z0-9]{20}$/);
    expect(r.voice_id).toBe(voiceCloneHappy.voice_id);
  });

  it("voice_clone_402_quota: cloneVoice throws with quota reason surfaced in message", async () => {
    stubFetch(
      async () => new Response(JSON.stringify(voiceClone402), { status: 402 }),
    );
    await expect(
      cloneVoice("k", {
        name: "Host",
        audioBase64: AUDIO_B64,
        sourceLang: null,
      }),
    ).rejects.toThrow(/voice_limit_reached/);
  });

  it("voice_delete_happy: deleteRemoteVoice does not throw when body is { status: 'ok' }", async () => {
    stubFetch(
      async () => new Response(JSON.stringify(voiceDeleteHappy), { status: 200 }),
    );
    await deleteRemoteVoice("k", voiceCloneHappy.voice_id);
  });
});

describe("§0.5.1 fixture roundtrip — Stripe webhook envelope", () => {
  const secret = "whsec_test_fixture_secret_padding_to_length_ok";

  it("checkout_session_completed: envelope shape matches Stripe webhook contract", () => {
    expect(checkoutSessionCompleted.type).toBe("checkout.session.completed");
    expect(checkoutSessionCompleted.api_version).toMatch(/^\d{4}-\d{2}-\d{2}$/);
    expect(checkoutSessionCompleted.livemode).toBe(false);
    expect(typeof checkoutSessionCompleted.created).toBe("number");
    expect(checkoutSessionCompleted.data.object.id).toMatch(/^cs_test_/);
    expect(checkoutSessionCompleted.data.object.customer).toMatch(/^cus_/);
    expect(checkoutSessionCompleted.data.object.subscription).toMatch(/^sub_/);
  });

  it("checkout_session_completed: passes signature verification and returns type", async () => {
    const payload = JSON.stringify(checkoutSessionCompleted);
    const t = checkoutSessionCompleted.created;
    const v1 = await hmacSha256Hex(secret, `${t}.${payload}`);
    const result = await verifyStripeSignature(payload, `t=${t},v1=${v1}`, secret, t);
    expect(result).toEqual({ ok: true, eventType: "checkout.session.completed" });
  });

  it("invoice_payment_failed: carries attempt_count + next_payment_attempt for dunning path", () => {
    expect(invoicePaymentFailed.type).toBe("invoice.payment_failed");
    expect(typeof invoicePaymentFailed.data.object.next_payment_attempt).toBe(
      "number",
    );
    expect(invoicePaymentFailed.data.object.attempt_count).toBeGreaterThan(0);
  });
});

describe("§0.5.1 fixture roundtrip — YouTube Live Streaming API", () => {
  it("broadcast_insert + stream_insert: createYouTubeBroadcast reads cdn.ingestionInfo correctly", async () => {
    stubFetch(async (url) => {
      const u = url.toString();
      if (u.includes("/liveBroadcasts/bind")) {
        return new Response(JSON.stringify(broadcastInsertHappy), { status: 200 });
      }
      if (u.includes("/liveBroadcasts")) {
        return new Response(JSON.stringify(broadcastInsertHappy), { status: 200 });
      }
      if (u.includes("/liveStreams")) {
        return new Response(JSON.stringify(streamInsertHappy), { status: 200 });
      }
      return new Response("unexpected", { status: 500 });
    });

    const result = await createYouTubeBroadcast(youtubeEnv, {
      accessToken: "ya29.fake",
      title: "Brivva Live",
      scheduledStartTime: "2026-05-10T12:05:00.000Z",
      privacyStatus: "unlisted",
    });

    expect(result.broadcastId).toBe(broadcastInsertHappy.id);
    expect(result.broadcastId).toMatch(/^[A-Za-z0-9_-]{11}$/);
    expect(result.streamId).toBe(streamInsertHappy.id);
    expect(result.rtmpUrl).toBe(
      streamInsertHappy.cdn.ingestionInfo.rtmpsIngestionAddress,
    );
    expect(result.streamKey).toBe(
      streamInsertHappy.cdn.ingestionInfo.streamName,
    );
    expect(result.watchUrl).toBe(
      `https://www.youtube.com/watch?v=${broadcastInsertHappy.id}`,
    );
  });

  it("broadcast_403_quota: surfaces as YouTubeBroadcastError with status 403", async () => {
    stubFetch(
      async () => new Response(JSON.stringify(broadcast403), { status: 403 }),
    );
    await expect(
      createYouTubeBroadcast(youtubeEnv, {
        accessToken: "t",
        title: "t",
        scheduledStartTime: "2026-05-10T12:05:00.000Z",
        privacyStatus: "unlisted",
      }),
    ).rejects.toBeInstanceOf(YouTubeBroadcastError);
  });
});

describe("§0.5.1 fixture roundtrip — Google OAuth (sign-in)", () => {
  it("signin_token_exchange: exchangeCode returns ya29.* access_token + 3599 expires_in", async () => {
    stubFetch(
      async () => new Response(JSON.stringify(signinTokenExchange), { status: 200 }),
    );
    const r = await exchangeSignin(signinEnv, "code");
    expect(r.access_token).toMatch(/^ya29\./);
    expect(r.expires_in).toBe(3599);
    expect(r.scope).toContain(" ");
    expect(r.id_token).toMatch(/^[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+$/);
  });

  it("signin_userinfo: fetchUserInfo returns sub + email + picture", async () => {
    stubFetch(
      async () => new Response(JSON.stringify(signinUserinfo), { status: 200 }),
    );
    const r = await fetchUserInfo("t");
    expect(r.sub).toBe(signinUserinfo.sub);
    expect(r.email).toBe(signinUserinfo.email);
    expect(r.picture).toBe(signinUserinfo.picture);
    expect(r.picture).toContain("googleusercontent.com");
  });

  it("signin_userinfo_missing_picture: picture normalized to null, not undefined", async () => {
    stubFetch(
      async () =>
        new Response(JSON.stringify(signinUserinfoMissingPic), { status: 200 }),
    );
    const r = await fetchUserInfo("t");
    expect(r.picture).toBeNull();
    expect(r.sub).toBe(signinUserinfoMissingPic.sub);
  });
});

describe("§0.5.1 fixture roundtrip — Google OAuth (YouTube scope)", () => {
  it("youtube_token_exchange: refresh_token prefixed 1// and scope contains youtube", async () => {
    stubFetch(
      async () =>
        new Response(JSON.stringify(youtubeTokenExchange), { status: 200 }),
    );
    const r = await exchangeYouTube(youtubeEnv, "code");
    expect(r.refresh_token).toMatch(/^1\/\//);
    expect(r.scope).toContain("youtube");
    expect(r.expires_in).toBe(3599);
  });

  it("invalid_grant: refreshAccessToken throws with invalid_grant in message", async () => {
    stubFetch(
      async () => new Response(JSON.stringify(invalidGrant), { status: 400 }),
    );
    await expect(refreshAccessToken(youtubeEnv, "revoked-refresh")).rejects.toThrow(
      /invalid_grant/,
    );
  });
});

describe("§0.5.1 fixture roundtrip — Grip Seller API (HAND_CRAFTED_PENDING_REAL_CAPTURE)", () => {
  // These fixtures are synthesized from a stub parser; real Grip Seller
  // API shape is gated on seller_support@gripcorp.co / cloud.bd@gripcorp.co.
  // When the real spec arrives, regenerate fixtures from a live capture
  // and these tests + the seller-api.ts parser must be updated together.
  it("provision_stream_happy: provisionBroadcast extracts rtmpUrl + streamKey + broadcastId", async () => {
    stubFetch(
      async () => new Response(JSON.stringify(gripProvisionHappy), { status: 200 }),
    );
    const r = await provisionBroadcast({} as Env, {
      accessKey: "ak",
      secretKey: "sk",
      productId: "prod_1",
    });
    expect(r.rtmpUrl).toBe(gripProvisionHappy.ingest.url);
    expect(r.streamKey).toBe(gripProvisionHappy.ingest.stream_key);
    expect(r.broadcastId).toBe(gripProvisionHappy.id);
    expect(r.rtmpUrl).toMatch(/^rtmps:\/\/.+:443/);
  });

  it("provision_stream_401: surfaces as GripSellerApiError with 401", async () => {
    stubFetch(
      async () =>
        new Response(JSON.stringify(gripProvision401), { status: 401 }),
    );
    await expect(
      provisionBroadcast({} as Env, {
        accessKey: "bad",
        secretKey: "bad",
        productId: "prod_1",
      }),
    ).rejects.toBeInstanceOf(GripSellerApiError);
  });
});
