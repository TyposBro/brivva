// §0.5.3 — Multi-step integration scenarios.
//
// Single-endpoint round-trips (covered by other test files) are necessary
// but not sufficient. This file chains API calls the way a real user
// journey does, then asserts final DB state + background-loop behavior
// against the mutation history.
//
// The 2026-04-20 production incident that motivated soft-end
// (commits b960367 + 3055ccc + b3542f4) was a multi-step interaction:
// the host hit stop → the session got soft-ended → the Fargate metrics
// loop fired one more PATCH between stop and the final cleanup signal →
// Workers returned 404 → server-rs retried in a tight loop and hit
// SQLITE_BUSY. Covering single endpoints never caught that graph; only
// chaining does.

import { describe, it, expect } from "vitest";
import { env } from "cloudflare:test";

import app from "../src/orchestration/app";

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

async function createSession(userId: string, targetLangs: string[]): Promise<string> {
  await seedUser(userId);
  const res = await call("/api/sessions", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      user_id: userId,
      title: "multi-step",
      source_lang: "en",
      target_langs: targetLangs,
    }),
  });
  const { session } = (await res.json()) as { session: { id: string } };
  return session.id;
}

async function addStream(
  sessionId: string,
  lang: string,
  platform: string,
): Promise<{ id: string }> {
  const res = await call(`/api/sessions/${sessionId}/streams`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      lang,
      platform,
      rtmp_url: `rtmp://ingest.${platform}/live`,
      stream_key: `sk_${platform}_${lang}_${Date.now()}`,
    }),
  });
  expect(res.status).toBe(200);
  return (await res.json()) as { id: string };
}

async function patchLive(
  sessionId: string,
  liveSessionId: string,
): Promise<Response> {
  return call(`/internal/sessions/${sessionId}`, {
    method: "PATCH",
    headers: {
      "Content-Type": "application/json",
      "X-Internal-Secret": env.INTERNAL_SECRET,
    },
    body: JSON.stringify({ status: "live", live_session_id: liveSessionId }),
  });
}

async function patchMetrics(
  sessionId: string,
  sourceSeconds: number,
  outputByLang: Record<string, number>,
): Promise<Response> {
  return call(`/internal/sessions/${sessionId}/metrics`, {
    method: "PATCH",
    headers: {
      "Content-Type": "application/json",
      "X-Internal-Secret": env.INTERNAL_SECRET,
    },
    body: JSON.stringify({
      source_seconds: sourceSeconds,
      output_seconds_by_lang: outputByLang,
    }),
  });
}

async function getInternal(id: string): Promise<{
  session: {
    status: string;
    live_session_id: string | null;
  };
  streams: Array<{ id: string; lang: string; platform: string }>;
}> {
  const res = await call(`/internal/sessions/${id}`, {
    headers: { "X-Internal-Secret": env.INTERNAL_SECRET },
  });
  return (await res.json()) as {
    session: { status: string; live_session_id: string | null };
    streams: Array<{ id: string; lang: string; platform: string }>;
  };
}

async function getUsage(id: string): Promise<{
  source_minutes: number;
  output_minutes_by_lang: Record<string, number>;
}> {
  const res = await call(`/api/sessions/${id}/usage`);
  const json = (await res.json()) as {
    source_minutes: number;
    output_minutes_by_lang: Record<string, number>;
  };
  return json;
}

describe("§0.5.3 multi-step integration — full user journey", () => {
  it("create → add stream → go live → add stream mid-live → metrics → soft-end → post-end metrics all land consistently", async () => {
    // 1. CREATE. One target lang up front (ja). Second lang (ko) will be
    //    added mid-session via a separate endpoint — proves session state
    //    can grow during a broadcast, not only at create time.
    const sessionId = await createSession("u-multistep-1", ["ja"]);

    // 2. ADD STREAM (pre-live). Destination for ja.
    const jaStream = await addStream(sessionId, "ja", "tiktok");
    expect(jaStream.id).toBeTruthy();

    // Intermediate check: session is still in setup, streams list now has 1.
    {
      const before = await getInternal(sessionId);
      expect(before.session.status).toBe("setup");
      expect(before.session.live_session_id).toBeNull();
      expect(before.streams.length).toBe(1);
      expect(before.streams[0]?.lang).toBe("ja");
    }

    // 3. GO LIVE (Fargate PATCH after the WS upgrade).
    const goLive = await patchLive(sessionId, "ROOM_MS_1");
    expect(goLive.status).toBe(200);

    // 4. FIRST METRICS PATCH. Two minutes of source audio, ja output caught
    //    up.
    const firstMetrics = await patchMetrics(sessionId, 120, { ja: 118 });
    expect(firstMetrics.status).toBe(200);

    // 5. MID-LIVE MUTATION. Host adds a Chinese destination 3 minutes in.
    //    Streams mutation while live is the exact race shape that needs
    //    multi-step coverage: single-endpoint tests can't catch a schema
    //    mismatch between add-stream and the live-status branch of the
    //    session read path.
    const koStream = await addStream(sessionId, "ko", "youtube");

    {
      const midLive = await getInternal(sessionId);
      expect(midLive.session.status).toBe("live");
      expect(midLive.session.live_session_id).toBe("ROOM_MS_1");
      expect(midLive.streams.length).toBe(2);
      const langs = midLive.streams.map((s) => s.lang).sort();
      expect(langs).toEqual(["ja", "ko"]);
      // Stream id we just inserted shows up in the session bundle.
      expect(midLive.streams.map((s) => s.id)).toContain(koStream.id);
    }

    // 6. SECOND METRICS PATCH after the ko stream landed. Reporter
    //    accumulates both langs; upsert merge-semantics must preserve ja.
    const secondMetrics = await patchMetrics(sessionId, 360, { ja: 355, ko: 180 });
    expect(secondMetrics.status).toBe(200);

    // 7. SOFT-END via DELETE. Row stays; status flips to ended.
    const deleteRes = await call(`/api/sessions/${sessionId}`, { method: "DELETE" });
    expect(deleteRes.status).toBe(200);

    {
      const afterEnd = await getInternal(sessionId);
      expect(afterEnd.session.status).toBe("ended");
      expect(afterEnd.session.live_session_id).toBeNull();
      // Streams are kept for the post-stream summary modal.
      expect(afterEnd.streams.length).toBe(2);
    }

    // 8. POST-END METRICS PATCH. The Fargate reporter fires ONE more tick
    //    between the DELETE and its own self-cancel on 404. Soft-end means
    //    the row is still there, so metrics merge cleanly — no 404, no
    //    SQLITE_BUSY.
    const postEndMetrics = await patchMetrics(sessionId, 380, { ja: 375, ko: 200 });
    expect(postEndMetrics.status).toBe(200);

    // 9. FINAL USAGE ROLLUP. Seconds → minutes conversion + per-lang split
    //    must reflect the LAST metrics upsert (not the pre-end first one).
    const usage = await getUsage(sessionId);
    // 380s / 60 = 6.33 min (rounded to 2dp).
    expect(usage.source_minutes).toBeCloseTo(6.33, 2);
    expect(usage.output_minutes_by_lang).toEqual({
      ja: expect.closeTo(6.25, 2), // 375/60
      ko: expect.closeTo(3.33, 2), // 200/60
    });
  });

  it("metrics reporter firing after DELETE on a never-created session id still returns 404 (self-cancel signal)", async () => {
    // Adjacent to the full-journey test above: the signal server-rs
    // relies on to self-cancel the metrics loop is "404 for never-existed
    // session id". Soft-end changed the signal for EXISTING sessions
    // (200 now), but the never-existed path must still 404 so the
    // reporter doesn't spin on a typo / race-condition session_id.
    const res = await patchMetrics("session-does-not-exist", 10, { ja: 10 });
    expect(res.status).toBe(404);
  });
});
