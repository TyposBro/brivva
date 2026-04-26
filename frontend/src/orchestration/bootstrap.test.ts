import { afterEach, describe, expect, it, vi } from "vitest";
import { resolveVideoIngest } from "./bootstrap";

describe("resolveVideoIngest", () => {
  afterEach(() => {
    vi.unstubAllEnvs();
    window.history.pushState({}, "", "/");
  });

  it("defaults to JPEG ingest", () => {
    window.history.pushState({}, "", "/");
    expect(resolveVideoIngest()).toBe("jpeg");
  });

  it("allows URL opt-in to WebRTC ingest", () => {
    window.history.pushState({}, "", "/?ingest=webrtc");
    expect(resolveVideoIngest()).toBe("webrtc");
  });

  it("allows env opt-in to WebRTC ingest", () => {
    vi.stubEnv("VITE_VIDEO_INGEST", "webrtc");
    expect(resolveVideoIngest()).toBe("webrtc");
  });
});
