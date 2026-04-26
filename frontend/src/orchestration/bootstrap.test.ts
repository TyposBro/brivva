import { afterEach, describe, expect, it, vi } from "vitest";
import { resolveVideoIngest } from "./bootstrap";

describe("resolveVideoIngest", () => {
  afterEach(() => {
    vi.unstubAllEnvs();
    window.history.pushState({}, "", "/");
  });

  it("defaults to WebRTC", () => {
    window.history.pushState({}, "", "/");
    expect(resolveVideoIngest()).toBe("webrtc");
  });

  it("allows URL fallback to legacy JPEG ingest", () => {
    window.history.pushState({}, "", "/?ingest=jpeg");
    expect(resolveVideoIngest()).toBe("jpeg");
  });

  it("allows env fallback to legacy JPEG ingest", () => {
    vi.stubEnv("VITE_VIDEO_INGEST", "jpeg");
    expect(resolveVideoIngest()).toBe("jpeg");
  });
});
