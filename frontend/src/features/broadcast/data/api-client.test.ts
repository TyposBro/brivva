import { describe, it, expect } from "vitest";
import { detectPlatform, langLabel, langFlag } from "./api-client";

describe("detectPlatform", () => {
  it("Instagram RTMPS URL — extracts streamKey after base", () => {
    const out = detectPlatform("rtmps://live-upload.instagram.com:443/rtmp/abc123");
    expect(out).toEqual({
      platform: "instagram",
      rtmpUrl: "rtmps://live-upload.instagram.com:443/rtmp/",
      streamKey: "abc123",
    });
  });

  it("Twitch URL", () => {
    const out = detectPlatform("rtmp://live.twitch.tv/app/live_xyz");
    expect(out).toEqual({
      platform: "twitch",
      rtmpUrl: "rtmp://live.twitch.tv/app/",
      streamKey: "live_xyz",
    });
  });

  it("Kuaishou URL", () => {
    const out = detectPlatform("rtmp://live.kuaishou.com/live/key_42");
    expect(out?.platform).toBe("kuaishou");
    expect(out?.streamKey).toBe("key_42");
  });

  it("Bilibili URL", () => {
    const out = detectPlatform("rtmp://live-push.bilivideo.com/live-bvc/bili_key");
    expect(out?.platform).toBe("bilibili");
    expect(out?.streamKey).toBe("bili_key");
  });

  it("YouTube RTMP", () => {
    const out = detectPlatform("rtmp://a.rtmp.youtube.com/live2/yt_key");
    expect(out?.platform).toBe("youtube");
    expect(out?.streamKey).toBe("yt_key");
  });

  it("YouTube RTMPS", () => {
    const out = detectPlatform("rtmps://a.rtmps.youtube.com/live2/yt_s_key");
    expect(out?.platform).toBe("youtube");
    expect(out?.streamKey).toBe("yt_s_key");
  });

  it("prefix match but different base path — falls back to last segment", () => {
    const out = detectPlatform("rtmp://live.twitch.tv/different/path/k");
    expect(out?.platform).toBe("twitch");
    expect(out?.streamKey).toBe("k");
  });

  it("custom rtmp:// — splits at last slash", () => {
    const out = detectPlatform("rtmp://my.server.com/app/some_key");
    expect(out).toEqual({
      platform: "custom",
      rtmpUrl: "rtmp://my.server.com/app/",
      streamKey: "some_key",
    });
  });

  it("custom rtmps://", () => {
    const out = detectPlatform("rtmps://edge.x.io/path/abc");
    expect(out?.platform).toBe("custom");
  });

  it("trims whitespace", () => {
    const out = detectPlatform("  rtmp://live.twitch.tv/app/key  ");
    expect(out?.platform).toBe("twitch");
    expect(out?.streamKey).toBe("key");
  });

  it("empty string → null (sad)", () => {
    expect(detectPlatform("")).toBeNull();
  });

  it("whitespace-only → null (sad)", () => {
    expect(detectPlatform("   ")).toBeNull();
  });

  it("non-RTMP string → null (sad)", () => {
    expect(detectPlatform("https://youtube.com/watch?v=123")).toBeNull();
  });

  it("bare word → null (sad)", () => {
    expect(detectPlatform("justAStreamKey")).toBeNull();
  });
});

describe("langLabel", () => {
  it("known codes return label", () => {
    expect(langLabel("ko")).toBe("Korean");
    expect(langLabel("en")).toBe("English");
    expect(langLabel("ja")).toBe("Japanese");
    expect(langLabel("zh")).toBe("Chinese");
  });

  it("unknown code → returns code (fallback)", () => {
    expect(langLabel("xx")).toBe("xx");
  });

  it("empty string → empty string", () => {
    expect(langLabel("")).toBe("");
  });
});

describe("langFlag", () => {
  it("known codes return flag emoji", () => {
    expect(langFlag("ko")).toBe("\uD83C\uDDF0\uD83C\uDDF7");
    expect(langFlag("en")).toBe("\uD83C\uDDEC\uD83C\uDDE7");
    expect(langFlag("ja")).toBe("\uD83C\uDDEF\uD83C\uDDF5");
    expect(langFlag("zh")).toBe("\uD83C\uDDE8\uD83C\uDDF3");
  });

  it("unknown code → empty string (fallback)", () => {
    expect(langFlag("xx")).toBe("");
  });
});
