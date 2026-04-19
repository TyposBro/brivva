import { describe, it, expect } from "vitest";
import { env } from "cloudflare:test";

import * as db from "../src/shared/db/db";

// D1 migrations applied + tables wiped by tests/setup.ts before each test.

describe("db.users", () => {
  it("getOrCreateUser inserts a fresh row when the id is new", async () => {
    const user = await db.getOrCreateUser(env.DB, "user-new");
    expect(user.id).toBe("user-new");
    expect(user.youtube_access_token).toBeNull();
    expect(user.youtube_refresh_token).toBeNull();
    expect(user.created_at).toBeGreaterThan(0);
  });

  it("getOrCreateUser is idempotent — second call returns the same row", async () => {
    const first = await db.getOrCreateUser(env.DB, "user-dup");
    const second = await db.getOrCreateUser(env.DB, "user-dup");
    expect(second.id).toBe(first.id);
    expect(second.created_at).toBe(first.created_at);
  });

  it("updateYouTubeTokens persists all fields and leaves `id` intact", async () => {
    await db.getOrCreateUser(env.DB, "user-yt");
    await db.updateYouTubeTokens(
      env.DB,
      "user-yt",
      "access-xyz",
      "refresh-abc",
      1_800_000_000,
      "UC-channel-id",
      "My Channel",
    );
    const u = await db.getOrCreateUser(env.DB, "user-yt");
    expect(u.youtube_access_token).toBe("access-xyz");
    expect(u.youtube_refresh_token).toBe("refresh-abc");
    expect(u.youtube_token_expires_at).toBe(1_800_000_000);
    expect(u.youtube_channel_id).toBe("UC-channel-id");
    expect(u.youtube_channel_name).toBe("My Channel");
  });

  it("updateAccessToken rotates only the access half, refresh is preserved", async () => {
    await db.getOrCreateUser(env.DB, "user-r");
    await db.updateYouTubeTokens(
      env.DB,
      "user-r",
      "access-1",
      "refresh-keep",
      1_000,
      "chan",
      "Name",
    );
    await db.updateAccessToken(env.DB, "user-r", "access-2", 2_000);
    const u = await db.getOrCreateUser(env.DB, "user-r");
    expect(u.youtube_access_token).toBe("access-2");
    expect(u.youtube_token_expires_at).toBe(2_000);
    expect(u.youtube_refresh_token).toBe("refresh-keep");
  });
});

describe("db.voices", () => {
  it("create → list → get → delete roundtrip", async () => {
    await db.getOrCreateUser(env.DB, "u-v");
    const v = await db.createVoice(env.DB, "u-v", "el-abc", "Host Voice");
    expect(v.id).toBeTruthy();

    const list = await db.listVoices(env.DB, "u-v");
    expect(list).toHaveLength(1);
    expect(list[0].name).toBe("Host Voice");

    const got = await db.getVoice(env.DB, v.id);
    expect(got?.elevenlabs_voice_id).toBe("el-abc");

    await db.deleteVoiceRow(env.DB, v.id);
    expect(await db.getVoice(env.DB, v.id)).toBeNull();
    expect(await db.listVoices(env.DB, "u-v")).toHaveLength(0);
  });

  it("listVoices is scoped to user_id — one user does not see another's voices", async () => {
    await db.getOrCreateUser(env.DB, "user-a");
    await db.getOrCreateUser(env.DB, "user-b");
    await db.createVoice(env.DB, "user-a", "el-a", "A-voice");
    await db.createVoice(env.DB, "user-b", "el-b", "B-voice");

    const aList = await db.listVoices(env.DB, "user-a");
    expect(aList).toHaveLength(1);
    expect(aList[0].name).toBe("A-voice");
  });

  it("getVoice returns null for an unknown id (sad path)", async () => {
    expect(await db.getVoice(env.DB, "no-such-id")).toBeNull();
  });
});

describe("db.sessions + streams", () => {
  it("createSession stores source_lang + target_langs JSON with default status", async () => {
    await db.getOrCreateUser(env.DB, "u-s");
    const s = await db.createSession(
      env.DB,
      "u-s",
      null,
      "Demo",
      "en",
      JSON.stringify(["ja", "ko"]),
    );
    expect(s.status).toBe("setup");
    expect(s.live_session_id).toBeNull();
    expect(JSON.parse(s.target_langs)).toEqual(["ja", "ko"]);
  });

  it("updateSessionStatus flips status + live_session_id", async () => {
    await db.getOrCreateUser(env.DB, "u-s");
    const s = await db.createSession(env.DB, "u-s", null, "D", "en", "[]");

    await db.updateSessionStatus(env.DB, s.id, "live", "ROOM01");
    const live = await db.getSession(env.DB, s.id);
    expect(live?.status).toBe("live");
    expect(live?.live_session_id).toBe("ROOM01");

    await db.updateSessionStatus(env.DB, s.id, "ended", null);
    const ended = await db.getSession(env.DB, s.id);
    expect(ended?.status).toBe("ended");
    expect(ended?.live_session_id).toBeNull();
  });

  it("deleteSessionRow cascades: child streams are removed with the session", async () => {
    await db.getOrCreateUser(env.DB, "u-s");
    const s = await db.createSession(env.DB, "u-s", null, "D", "en", "[]");
    await db.createStreamManual(env.DB, s.id, "ja", "twitch", "rtmp://x", "k1", 2000, 0.2);
    await db.createStreamManual(env.DB, s.id, "ko", "twitch", "rtmp://y", "k2", 2000, 0.2);

    expect(await db.listStreams(env.DB, s.id)).toHaveLength(2);

    await db.deleteSessionRow(env.DB, s.id);
    expect(await db.getSession(env.DB, s.id)).toBeNull();
    expect(await db.listStreams(env.DB, s.id)).toHaveLength(0);
  });

  it("createStreamManual persists delay_ms + host_gain passed in", async () => {
    await db.getOrCreateUser(env.DB, "u-s");
    const s = await db.createSession(env.DB, "u-s", null, "D", "en", "[]");
    const stream = await db.createStreamManual(
      env.DB,
      s.id,
      "ja",
      "twitch",
      "rtmp://host",
      "secret-key",
      1500,
      0.35,
    );
    expect(stream.delay_ms).toBe(1500);
    expect(stream.host_gain).toBeCloseTo(0.35);
    expect(stream.status).toBe("ready");
  });

  it("listStreams is scoped per session_id — sessions don't leak streams to each other", async () => {
    await db.getOrCreateUser(env.DB, "u-s");
    const s1 = await db.createSession(env.DB, "u-s", null, "One", "en", "[]");
    const s2 = await db.createSession(env.DB, "u-s", null, "Two", "en", "[]");
    await db.createStreamManual(env.DB, s1.id, "ja", "yt", "rtmp://a", "k", 2000, 0.2);
    await db.createStreamManual(env.DB, s2.id, "ko", "tw", "rtmp://b", "k", 2000, 0.2);

    const s1Streams = await db.listStreams(env.DB, s1.id);
    expect(s1Streams).toHaveLength(1);
    expect(s1Streams[0].lang).toBe("ja");
  });

  it("updateStreamPlatform flips status to ready and fills YouTube-generated ids", async () => {
    await db.getOrCreateUser(env.DB, "u-s");
    const s = await db.createSession(env.DB, "u-s", null, "D", "en", "[]");
    const stream = await db.createStreamManual(env.DB, s.id, "ja", "twitch", "rtmp://x", "", 2000, 0.2);
    await db.updateStreamPlatform(
      env.DB,
      stream.id,
      "YT-BROADCAST-1",
      "YT-STREAM-1",
      "new-key",
      "rtmp://youtube.example",
    );
    const list = await db.listStreams(env.DB, s.id);
    expect(list[0].platform_broadcast_id).toBe("YT-BROADCAST-1");
    expect(list[0].stream_key).toBe("new-key");
    expect(list[0].status).toBe("ready");
  });

  it("getSession returns null for an unknown id (sad path)", async () => {
    expect(await db.getSession(env.DB, "does-not-exist")).toBeNull();
  });
});

describe("db.platform_credentials", () => {
  it("upsertCredential inserts a fresh row when (user, platform) is new", async () => {
    const row = await db.upsertCredential(
      env.DB,
      "u-c",
      "twitch",
      "rtmp://live.tw",
      "sk-1",
      "Main",
    );
    expect(row.platform).toBe("twitch");
    expect(row.display_name).toBe("Main");
  });

  it("upsertCredential updates existing row (unique by user+platform) rather than duplicating", async () => {
    await db.upsertCredential(env.DB, "u-c", "twitch", "rtmp://a", "k-old", "Old name");
    await db.upsertCredential(env.DB, "u-c", "twitch", "rtmp://b", "k-new", "New name");

    const list = await db.listCredentials(env.DB, "u-c");
    expect(list).toHaveLength(1);
    expect(list[0].stream_key).toBe("k-new");
    expect(list[0].rtmp_url).toBe("rtmp://b");
    expect(list[0].display_name).toBe("New name");
  });

  it("upsert keeps the prior display_name when the new one is null (COALESCE behavior)", async () => {
    await db.upsertCredential(env.DB, "u-c", "twitch", "rtmp://a", "k", "Keep me");
    await db.upsertCredential(env.DB, "u-c", "twitch", "rtmp://a", "k", null);
    const list = await db.listCredentials(env.DB, "u-c");
    expect(list[0].display_name).toBe("Keep me");
  });

  it("deleteCredentialRow removes only the matching (user, platform)", async () => {
    await db.upsertCredential(env.DB, "u-c", "twitch", "rtmp://a", "k", null);
    await db.upsertCredential(env.DB, "u-c", "youtube", "rtmp://b", "k", null);
    await db.upsertCredential(env.DB, "u-c2", "twitch", "rtmp://c", "k", null);

    await db.deleteCredentialRow(env.DB, "u-c", "twitch");

    expect(await db.listCredentials(env.DB, "u-c")).toHaveLength(1);
    expect(await db.listCredentials(env.DB, "u-c2")).toHaveLength(1);
  });

  it("getCredential returns null for a missing platform (sad path)", async () => {
    expect(await db.getCredential(env.DB, "u-c", "missing")).toBeNull();
  });
});
