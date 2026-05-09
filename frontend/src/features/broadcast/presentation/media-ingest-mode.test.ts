import { describe, expect, it, beforeEach, vi } from "vitest";
import { _setAppConfig } from "../../../core/config/app-config";
import {
  MEDIA_INGEST_MODE_STORAGE_KEY,
  detectWebCodecsSupport,
  parseServerCapabilities,
  readStoredMediaIngestMode,
  resolveMediaIngestMode,
  webCodecsRecordBlocker,
  writeStoredMediaIngestMode,
} from "./media-ingest-mode";

class FakeStorage {
  private store = new Map<string, string>();
  getItem(k: string) { return this.store.get(k) ?? null; }
  setItem(k: string, v: string) { this.store.set(k, String(v)); }
  removeItem(k: string) { this.store.delete(k); }
}

function setConfig(webCodecsIngestEnabled: boolean) {
  _setAppConfig({
    workersApiBase: "http://test.invalid",
    mediaWsBase: "ws://test.invalid",
    sessionLogsEnabled: false,
    sessionLogConsole: false,
    timestampedAudioEnabled: false,
    webRtcIceServers: null,
    webRtcTurnCredentialsEnabled: false,
    webCodecsIngestEnabled,
  });
}

describe("media ingest mode helpers", () => {
  beforeEach(() => {
    vi.unstubAllGlobals();
    vi.stubGlobal("localStorage", new FakeStorage());
    setConfig(false);
  });

  it("persists valid mode and falls back to auto for invalid storage", () => {
    expect(readStoredMediaIngestMode()).toBe("auto");
    localStorage.setItem(MEDIA_INGEST_MODE_STORAGE_KEY, "bad");
    expect(readStoredMediaIngestMode()).toBe("auto");
    writeStoredMediaIngestMode("webcodecs_ws");
    expect(readStoredMediaIngestMode()).toBe("webcodecs_ws");
  });

  it("resolves auto to WebRTC until WebCodecs soak passes", () => {
    expect(resolveMediaIngestMode("auto")).toBe("webrtc");
    expect(resolveMediaIngestMode("webrtc")).toBe("webrtc");
    expect(resolveMediaIngestMode("webcodecs_ws")).toBe("webcodecs_ws");
  });

  it("detects required WebCodecs browser APIs", () => {
    expect(detectWebCodecsSupport().available).toBe(false);
    vi.stubGlobal("VideoEncoder", class {});
    vi.stubGlobal("VideoFrame", class {});
    vi.stubGlobal("MediaStreamTrackProcessor", class {});
    expect(detectWebCodecsSupport()).toEqual({ available: true, reasons: [] });
  });

  it("blocks WebCodecs recording until frontend, browser, and server caps agree", () => {
    expect(webCodecsRecordBlocker("webcodecs_ws", null)).toContain("frontend build");
    setConfig(true);
    expect(webCodecsRecordBlocker("webcodecs_ws", null)).toContain("not available");
    vi.stubGlobal("VideoEncoder", class {});
    vi.stubGlobal("VideoFrame", class {});
    vi.stubGlobal("MediaStreamTrackProcessor", class {});
    expect(webCodecsRecordBlocker("webcodecs_ws", null)).toContain("Checking");
    expect(
      webCodecsRecordBlocker("webcodecs_ws", {
        videoIngestModes: ["webrtc"],
        webcodecsCodecs: [],
      }),
    ).toContain("disabled on this server");
    expect(
      webCodecsRecordBlocker("webcodecs_ws", {
        videoIngestModes: ["webrtc", "webcodecs_ws"],
        webcodecsCodecs: ["vp8"],
      }),
    ).toBeNull();
  });

  it("parses server capabilities and ignores unrelated messages", () => {
    expect(parseServerCapabilities({ type: "noop" })).toBeNull();
    expect(
      parseServerCapabilities({
        type: "server:capabilities",
        videoIngestModes: ["webrtc", "bad", "webcodecs_ws"],
        webcodecsCodecs: ["vp8", 7],
      }),
    ).toEqual({ videoIngestModes: ["webrtc", "webcodecs_ws"], webcodecsCodecs: ["vp8"] });
  });
});
