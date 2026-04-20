// Contract round-trip tests — prove the Workers route responses match the
// FE's canonical Zod schemas for the 5 highest-traffic endpoints.
//
// Motivation — the 2026-04-19 `cost_usd` / `estimated_cost_usd` drift:
//   Workers was returning `{ cost_usd: 45 }`; the FE `SessionQuoteResponseSchema`
//   expected `{ estimated_cost_usd: 45 }`. Line coverage was 99% on both sides
//   because each side tested against its own hand-written fixture — neither
//   crossed the wire. The raw Zod issues blob hit production as a result.
//
// How this suite differs from api.test.ts:
//   - api.test.ts covers business logic (status codes, side effects, idempotency,
//     edge branches). It hand-picks the fields it cares about and is allowed to
//     assert against an ad-hoc shape.
//   - This file does ONE thing per endpoint: hit the real handler, pipe the
//     response body through the REAL `@brivva/contracts/http` schema, and fail
//     loudly if .safeParse() returns success=false. No hand-written fixtures.
//
// If a handler renames/drops a field, this suite is the tripwire.
//
// We do NOT re-cover status codes, 400s, auth, or fetch-stub paths here — those
// belong in api.test.ts. Scope-creep would bloat the suite and dilute its intent.

import { describe, it, expect, vi, afterEach } from "vitest";
import { env } from "cloudflare:test";
import {
  CreateSessionResponseSchema,
  ListCredentialsResponseSchema,
  SessionQuoteResponseSchema,
  SessionSummaryResponseSchema,
  UserInfoSchema,
  VoiceSchema,
} from "@brivva/contracts/http";

import app from "../src/orchestration/app";

// ── helpers ──────────────────────────────────────────────────

async function call(path: string, init?: RequestInit): Promise<Response> {
  const url = new URL(path, "https://test.local");
  return await app.fetch(new Request(url, init), env);
}

// Parse + surface the Zod issues verbatim on failure. Without this, a drift
// manifests as `expected true, got false` which tells the next engineer
// nothing about which field drifted.
function assertMatches<T>(
  schema: { safeParse(input: unknown): { success: true; data: T } | { success: false; error: { issues: unknown } } },
  body: unknown,
  label: string,
): void {
  const parsed = schema.safeParse(body);
  if (!parsed.success) {
    throw new Error(
      `${label} response does not match FE schema.\nIssues: ${JSON.stringify(
        parsed.error.issues,
        null,
        2,
      )}\nBody: ${JSON.stringify(body, null, 2)}`,
    );
  }
  expect(parsed.success).toBe(true);
}

// Minimal PCM WAV builder — duplicated from api.test.ts intentionally so these
// two test files stay independently runnable. Kept tiny: 4kHz / 8-bit / mono.
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
  view.setUint32(16, 16, true);
  view.setUint16(20, 1, true);
  view.setUint16(22, channels, true);
  view.setUint32(24, sampleRate, true);
  view.setUint32(28, sampleRate * channels * bytesPerSample, true);
  view.setUint16(32, channels * bytesPerSample, true);
  view.setUint16(34, bitsPerSample, true);
  ascii("data", 36);
  view.setUint32(40, dataSize, true);
  let binary = "";
  for (let i = 0; i < buf.length; i++) binary += String.fromCharCode(buf[i]!);
  return btoa(binary);
}

const VALID_WAV_B64 = makeWavBase64(32);

function stubElevenLabsClone(voiceId: string): void {
  vi.stubGlobal(
    "fetch",
    vi.fn(async (input: RequestInfo | URL): Promise<Response> => {
      const url =
        typeof input === "string"
          ? input
          : input instanceof URL
            ? input.toString()
            : input.url;
      if (/api\.elevenlabs\.io\/v1\/voices\/add/.test(url)) {
        return new Response(JSON.stringify({ voice_id: voiceId }), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        });
      }
      throw new Error(`unstubbed fetch in contract-roundtrip: ${url}`);
    }),
  );
}

afterEach(() => {
  vi.unstubAllGlobals();
});

// ── 1. GET /api/user — UserInfoSchema ────────────────────────

describe("contract-roundtrip GET /api/user", () => {
  it("fresh user response matches UserInfoSchema (happy)", async () => {
    const res = await call("/api/user?user_id=rt-fresh");
    expect(res.status).toBe(200);
    assertMatches(UserInfoSchema, await res.json(), "GET /api/user");
  });

  it("user with every nullable field populated matches UserInfoSchema (edge)", async () => {
    // Drift-prone state: every nullable UserInfo field carries a real value.
    // Catches a regression where a non-null column accidentally gets mapped
    // to `undefined` (Zod distinguishes; JSON serialisation drops undefined).
    const ts = Math.floor(Date.now() / 1000);
    await env.DB.prepare(
      `INSERT INTO users
         (id, youtube_channel_id, youtube_channel_name, youtube_refresh_token,
          email, name, picture, onboarding_completed_at, active_voice_id,
          billing_tier, bills_to, created_at)
       VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`,
    )
      .bind(
        "rt-full",
        "UCxyz",
        "Brivva Channel",
        "yt-refresh-token",
        "full@example.com",
        "Full User",
        "https://example.com/pic.png",
        ts,
        null, // active_voice_id wired separately once voices FK exists
        "b2b",
        "ACME Corp",
        ts,
      )
      .run();

    const res = await call("/api/user?user_id=rt-full");
    expect(res.status).toBe(200);
    assertMatches(UserInfoSchema, await res.json(), "GET /api/user (full)");
  });
});

// ── 2. POST /api/user/complete-onboarding — UserInfoSchema ───

describe("contract-roundtrip POST /api/user/complete-onboarding", () => {
  it("response after stamping matches UserInfoSchema (happy)", async () => {
    const res = await call("/api/user/complete-onboarding", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ user_id: "rt-onb-1" }),
    });
    expect(res.status).toBe(200);
    assertMatches(
      UserInfoSchema,
      await res.json(),
      "POST /api/user/complete-onboarding",
    );
  });

  it("idempotent second call still matches UserInfoSchema (edge)", async () => {
    // Re-POST on an already-onboarded user — FE calls this on every reload of
    // the legacy "Welcome" screen. Edge because the handler hits a DB row that
    // already carries a non-null `onboarding_completed_at`.
    await call("/api/user/complete-onboarding", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ user_id: "rt-onb-2" }),
    });
    const res = await call("/api/user/complete-onboarding", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ user_id: "rt-onb-2" }),
    });
    expect(res.status).toBe(200);
    assertMatches(
      UserInfoSchema,
      await res.json(),
      "POST /api/user/complete-onboarding (second call)",
    );
  });
});

// ── 3. POST /api/voices — VoiceSchema (bare row) ─────────────

describe("contract-roundtrip POST /api/voices", () => {
  it("with source_lang hint, response matches VoiceSchema (happy)", async () => {
    stubElevenLabsClone("el-rt-happy");
    const res = await call("/api/voices", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        user_id: "rt-voice-1",
        name: "RT Voice",
        audio_base64: VALID_WAV_B64,
        source_lang: "ko",
      }),
    });
    expect(res.status).toBe(200);
    assertMatches(VoiceSchema, await res.json(), "POST /api/voices (happy)");
  });

  it("without source_lang hint, response still matches VoiceSchema (edge)", async () => {
    // Pre-language-hint historical rows had `source_lang: null` — this
    // exercises the nullable branch of VoiceSchema where a JSON serialiser
    // could drop the field altogether instead of emitting `null`.
    stubElevenLabsClone("el-rt-edge");
    const res = await call("/api/voices", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        user_id: "rt-voice-2",
        name: "RT Voice Unlabeled",
        audio_base64: VALID_WAV_B64,
      }),
    });
    expect(res.status).toBe(200);
    assertMatches(VoiceSchema, await res.json(), "POST /api/voices (edge)");
  });
});

// ── 4. POST /api/sessions — CreateSessionResponseSchema ──────

describe("contract-roundtrip POST /api/sessions", () => {
  it("with platforms, response matches CreateSessionResponseSchema (happy)", async () => {
    const res = await call("/api/sessions", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        user_id: "rt-sess-1",
        title: "Round trip",
        source_lang: "ko",
        target_langs: ["en", "ja"],
        platforms: [
          {
            platform: "yt",
            lang: "en",
            rtmp_url: "rtmp://yt/en",
            stream_key: "en-key",
          },
        ],
      }),
    });
    expect(res.status).toBe(200);
    assertMatches(
      CreateSessionResponseSchema,
      await res.json(),
      "POST /api/sessions (happy)",
    );
  });

  it("without platforms, empty streams array still matches (edge)", async () => {
    // Onboarding path: users create a session before wiring any destination.
    // Drift-prone because `streams: []` is an easy place to accidentally emit
    // `undefined` or omit the key entirely.
    const res = await call("/api/sessions", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        user_id: "rt-sess-2",
        title: "No destinations yet",
        source_lang: "en",
        target_langs: ["ja"],
      }),
    });
    expect(res.status).toBe(200);
    assertMatches(
      CreateSessionResponseSchema,
      await res.json(),
      "POST /api/sessions (edge)",
    );
  });
});

// ── 5. GET /api/credentials — ListCredentialsResponseSchema ──

describe("contract-roundtrip GET /api/credentials", () => {
  it("user with credentials, response matches ListCredentialsResponseSchema (happy)", async () => {
    await call("/api/credentials", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        user_id: "rt-creds-1",
        platform: "twitch",
        rtmp_url: "rtmp://live.twitch.tv/app",
        stream_key: "live_123",
        display_name: "My Twitch",
      }),
    });
    const res = await call("/api/credentials?user_id=rt-creds-1");
    expect(res.status).toBe(200);
    assertMatches(
      ListCredentialsResponseSchema,
      await res.json(),
      "GET /api/credentials (happy)",
    );
  });

  it("user with zero credentials, empty list still matches (edge)", async () => {
    // First-load state before the user pastes any RTMP keys. Empty-array
    // responses are a classic schema-drift blind spot because most codebases
    // test the populated branch and forget the empty one.
    const res = await call("/api/credentials?user_id=rt-creds-empty");
    expect(res.status).toBe(200);
    assertMatches(
      ListCredentialsResponseSchema,
      await res.json(),
      "GET /api/credentials (edge)",
    );
  });
});

// ── 6. GET /api/sessions/:id/quote — SessionQuoteResponseSchema

describe("contract-roundtrip GET /api/sessions/:id/quote", () => {
  it("user with active voice clone, response matches SessionQuoteResponseSchema (happy)", async () => {
    // Regression guard for the exact 2026-04-19 drift: if a handler ever
    // emits `cost_usd` again instead of `estimated_cost_usd`, this parse
    // fails and the test message points directly at the renamed field.
    const ts = Math.floor(Date.now() / 1000);
    await env.DB.prepare("INSERT INTO users (id, created_at) VALUES (?, ?)")
      .bind("rt-quote-voiced", ts)
      .run();
    // source_lang="ko" aligns with the session body below so the strict
    // enrollment-vs-session guard in POST /api/sessions doesn't reject the
    // quote-setup call.
    await env.DB.prepare(
      "INSERT INTO voices (id, user_id, elevenlabs_voice_id, name, source_lang, created_at) VALUES (?, ?, ?, ?, ?, ?)",
    )
      .bind("v-rt-q", "rt-quote-voiced", "el-rt-q", "Voiced", "ko", ts)
      .run();
    await env.DB.prepare("UPDATE users SET active_voice_id = ? WHERE id = ?")
      .bind("v-rt-q", "rt-quote-voiced")
      .run();

    const createRes = await call("/api/sessions", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        user_id: "rt-quote-voiced",
        title: "Quote with voice",
        source_lang: "ko",
        target_langs: ["en", "ja"],
      }),
    });
    const { session } = (await createRes.json()) as { session: { id: string } };

    const res = await call(
      `/api/sessions/${session.id}/quote?expected_minutes=30`,
    );
    expect(res.status).toBe(200);
    assertMatches(
      SessionQuoteResponseSchema,
      await res.json(),
      "GET /api/sessions/:id/quote (happy)",
    );
  });

  it("fresh user without active voice, response still matches (edge)", async () => {
    // Exact production repro of the drift incident: user with no cloned voice
    // opens the quote modal on /dashboard. Pre-fix, the renamed field would
    // fail `safeParse` here. Keeping this edge case means any future rename
    // that silently defaults to a missing-field branch gets caught too.
    const createRes = await call("/api/sessions", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        user_id: "rt-quote-fresh",
        title: "Fresh user quote",
        source_lang: "ko",
        target_langs: ["ja"],
      }),
    });
    const { session } = (await createRes.json()) as { session: { id: string } };

    const res = await call(
      `/api/sessions/${session.id}/quote?expected_minutes=30`,
    );
    expect(res.status).toBe(200);
    assertMatches(
      SessionQuoteResponseSchema,
      await res.json(),
      "GET /api/sessions/:id/quote (edge)",
    );
  });
});

// ── 7. GET /api/sessions/:id/summary — SessionSummaryResponseSchema

describe("contract-roundtrip GET /api/sessions/:id/summary", () => {
  it("self_serve session response matches SessionSummaryResponseSchema (happy)", async () => {
    const createRes = await call("/api/sessions", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        user_id: "rt-sum-self",
        title: "Self-serve summary",
        source_lang: "ko",
        target_langs: ["ja"],
      }),
    });
    const { session } = (await createRes.json()) as { session: { id: string } };

    const res = await call(`/api/sessions/${session.id}/summary`);
    expect(res.status).toBe(200);
    assertMatches(
      SessionSummaryResponseSchema,
      await res.json(),
      "GET /api/sessions/:id/summary (self_serve)",
    );
  });

  it("b2b session response matches SessionSummaryResponseSchema (edge)", async () => {
    // billing_tier + bills_to populated: total_cost_usd/rate_usd must be
    // null, billed_to must be a string. Contract parse catches any drift.
    const ts = Math.floor(Date.now() / 1000);
    await env.DB.prepare(
      "INSERT INTO users (id, billing_tier, bills_to, created_at) VALUES (?, ?, ?, ?)",
    )
      .bind("rt-sum-b2b", "b2b", "Brivva Tech Studio", ts)
      .run();
    const createRes = await call("/api/sessions", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        user_id: "rt-sum-b2b",
        title: "B2B summary",
        source_lang: "ko",
        target_langs: ["ja"],
      }),
    });
    const { session } = (await createRes.json()) as { session: { id: string } };

    const res = await call(`/api/sessions/${session.id}/summary`);
    expect(res.status).toBe(200);
    assertMatches(
      SessionSummaryResponseSchema,
      await res.json(),
      "GET /api/sessions/:id/summary (b2b)",
    );
  });
});
