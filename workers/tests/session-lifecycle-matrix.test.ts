import { describe, it, expect, beforeEach } from "vitest";
import { env } from "cloudflare:test";

import app from "../src/orchestration/app";

// §0.5.2 — Lifecycle matrix for the session state machine.
//
// States observed: `setup` (default after POST), `live` (Fargate PATCH
// signals session handle is up), `ended` (DELETE soft-end or Fargate
// teardown PATCH).
//
// Each row below is one `(current_state, event) → next_state` assertion.
// The loose `z.string().min(1)` schema on status means Workers itself
// does not enforce transition legality — several tests here document
// ACTUAL behavior (e.g. ended → live currently succeeds) so a future
// tightening of the schema intentionally breaks them.

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

async function createSession(userId: string): Promise<string> {
  await seedUser(userId);
  const res = await call("/api/sessions", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      user_id: userId,
      title: "matrix",
      source_lang: "en",
      target_langs: ["ja"],
    }),
  });
  const { session } = (await res.json()) as { session: { id: string } };
  return session.id;
}

async function patchStatus(
  id: string,
  status: string,
  liveSessionId: string | null = null,
): Promise<Response> {
  return call(`/internal/sessions/${id}`, {
    method: "PATCH",
    headers: {
      "Content-Type": "application/json",
      "X-Internal-Secret": env.INTERNAL_SECRET,
    },
    body: JSON.stringify({ status, live_session_id: liveSessionId }),
  });
}

async function getInternal(id: string): Promise<{
  session: { status: string; live_session_id: string | null };
}> {
  const res = await call(`/internal/sessions/${id}`, {
    headers: { "X-Internal-Secret": env.INTERNAL_SECRET },
  });
  return (await res.json()) as {
    session: { status: string; live_session_id: string | null };
  };
}

describe("session lifecycle matrix §0.5.2", () => {
  describe("forward happy-path transitions", () => {
    it("setup → live marks status 'live' and stores live_session_id", async () => {
      const id = await createSession("u-matrix-1");

      const before = await getInternal(id);
      expect(before.session.status).toBe("setup");
      expect(before.session.live_session_id).toBeNull();

      const res = await patchStatus(id, "live", "ROOM_A");
      expect(res.status).toBe(200);

      const after = await getInternal(id);
      expect(after.session.status).toBe("live");
      expect(after.session.live_session_id).toBe("ROOM_A");
    });

    it("live → ended via DELETE soft-ends and clears live_session_id", async () => {
      const id = await createSession("u-matrix-2");
      await patchStatus(id, "live", "ROOM_B");

      const del = await call(`/api/sessions/${id}`, { method: "DELETE" });
      expect(del.status).toBe(200);

      const after = await getInternal(id);
      expect(after.session.status).toBe("ended");
      expect(after.session.live_session_id).toBeNull();
    });

    it("setup → ended (host cancels before going live)", async () => {
      const id = await createSession("u-matrix-3");

      const del = await call(`/api/sessions/${id}`, { method: "DELETE" });
      expect(del.status).toBe(200);

      const after = await getInternal(id);
      expect(after.session.status).toBe("ended");
      expect(after.session.live_session_id).toBeNull();
    });

    it("setup → live → ended (full normal lifecycle in one test)", async () => {
      const id = await createSession("u-matrix-4");
      await patchStatus(id, "live", "ROOM_C");
      await call(`/api/sessions/${id}`, { method: "DELETE" });

      const after = await getInternal(id);
      expect(after.session.status).toBe("ended");
    });
  });

  describe("idempotent repeats (§0.5.2 start→stop→start)", () => {
    it("live → live with same live_session_id is a no-op", async () => {
      const id = await createSession("u-matrix-5");
      await patchStatus(id, "live", "ROOM_D");
      const second = await patchStatus(id, "live", "ROOM_D");
      expect(second.status).toBe(200);

      const after = await getInternal(id);
      expect(after.session.status).toBe("live");
      expect(after.session.live_session_id).toBe("ROOM_D");
    });

    it("live → live with different live_session_id overwrites (last-write-wins — Fargate reconnect case)", async () => {
      // The b3542f4 fix evicts the stale live_session on WS reconnect so TTS
      // unblocks. This test documents the DB-level behavior: the second PATCH
      // wins, matching the memory semantics in server-rs.
      const id = await createSession("u-matrix-6");
      await patchStatus(id, "live", "ROOM_OLD");
      await patchStatus(id, "live", "ROOM_NEW");

      const after = await getInternal(id);
      expect(after.session.live_session_id).toBe("ROOM_NEW");
    });

    it("ended → ended via DELETE is idempotent and does not resurrect", async () => {
      const id = await createSession("u-matrix-7");
      await call(`/api/sessions/${id}`, { method: "DELETE" });
      const second = await call(`/api/sessions/${id}`, { method: "DELETE" });
      expect(second.status).toBe(200);

      const after = await getInternal(id);
      expect(after.session.status).toBe("ended");
    });

    it("DELETE on a never-created session id returns 200 (server-rs race path)", async () => {
      const res = await call("/api/sessions/never-existed", { method: "DELETE" });
      expect(res.status).toBe(200);
      const body = (await res.json()) as { status: string };
      expect(body.status).toBe("ended");
    });
  });

  describe("illegal / unexpected transitions (current behavior documentation)", () => {
    it("ended → live via internal PATCH currently succeeds — documents schema looseness", async () => {
      // Workers' InternalSessionStatusUpdateSchema validates only
      // `z.string().min(1)` on status. There is no transition guard. If this
      // test starts failing, someone has tightened the schema to enforce a
      // transition matrix — update the suite and decide whether ended→live
      // should be a 409 or just be silently dropped.
      const id = await createSession("u-matrix-8");
      await call(`/api/sessions/${id}`, { method: "DELETE" });

      const res = await patchStatus(id, "live", "ROOM_REVIVE");
      expect(res.status).toBe(200);

      const after = await getInternal(id);
      expect(after.session.status).toBe("live");
      expect(after.session.live_session_id).toBe("ROOM_REVIVE");
    });

    it("live → setup backwards PATCH currently succeeds — documents schema looseness", async () => {
      const id = await createSession("u-matrix-9");
      await patchStatus(id, "live", "ROOM_Y");

      const res = await patchStatus(id, "setup", null);
      expect(res.status).toBe(200);

      const after = await getInternal(id);
      expect(after.session.status).toBe("setup");
      expect(after.session.live_session_id).toBeNull();
    });

    it("PATCH with garbage status string is accepted today — schema has no enum", async () => {
      // Zod currently accepts any non-empty string; the DB stores what it's
      // given. This test exists so a future SessionStatusEnum fails loudly
      // right here instead of quietly shipping.
      const id = await createSession("u-matrix-10");
      const res = await patchStatus(id, "banana", null);
      expect(res.status).toBe(200);

      const after = await getInternal(id);
      expect(after.session.status).toBe("banana");
    });
  });

  describe("metrics after lifecycle end (§0.5.3 multi-step + §0.5.4 observability)", () => {
    it("metrics PATCH on an ended session succeeds (soft-end keeps the row)", async () => {
      // Pre-b960367, the metrics reporter in server-rs would hit 404 because
      // DELETE hard-dropped the row; reporter spammed retries → SQLITE_BUSY.
      // Soft-end means the row is still there and metrics still upsert.
      const id = await createSession("u-matrix-11");
      await patchStatus(id, "live", "ROOM_M");
      await call(`/api/sessions/${id}`, { method: "DELETE" });

      const res = await call(`/internal/sessions/${id}/metrics`, {
        method: "PATCH",
        headers: {
          "Content-Type": "application/json",
          "X-Internal-Secret": env.INTERNAL_SECRET,
        },
        body: JSON.stringify({
          source_seconds: 10,
          output_seconds_by_lang: { ja: 12 },
        }),
      });
      expect(res.status).toBe(200);
    });

    it("metrics PATCH on a never-created session id returns 404 → server-rs reporter must self-cancel on this signal", async () => {
      // Confirms the cancellation contract b960367 relies on: reporter sees
      // 404, kills its own loop instead of retrying.
      const res = await call("/internal/sessions/never-existed/metrics", {
        method: "PATCH",
        headers: {
          "Content-Type": "application/json",
          "X-Internal-Secret": env.INTERNAL_SECRET,
        },
        body: JSON.stringify({ source_seconds: 1 }),
      });
      expect(res.status).toBe(404);
    });
  });

  describe("concurrency (§0.5.2 concurrent start×2)", () => {
    it("two Fargate PATCHes racing on the same session id both land, last-write-wins", async () => {
      // Real case: Fargate spawns a new pod on restart while the old one
      // hasn't drained yet. Both PATCH live with different live_session_ids.
      // DB accepts both; the b3542f4 eviction logic lives in server-rs, not
      // here. Workers must not 500 on the race.
      const id = await createSession("u-matrix-12");

      const [first, second] = await Promise.all([
        patchStatus(id, "live", "ROOM_FIRST"),
        patchStatus(id, "live", "ROOM_SECOND"),
      ]);
      expect(first.status).toBe(200);
      expect(second.status).toBe(200);

      const after = await getInternal(id);
      expect(after.session.status).toBe("live");
      expect(["ROOM_FIRST", "ROOM_SECOND"]).toContain(
        after.session.live_session_id,
      );
    });
  });
});
