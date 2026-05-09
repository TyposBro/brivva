import "@testing-library/jest-dom/vitest";
import { afterEach, vi } from "vitest";
import { cleanup } from "@testing-library/react";
import { _setAppConfig } from "../core/config/app-config";

// Seed AppConfig so module-level calls to appConfig() inside features don't
// throw during test runs. Individual tests can re-seed with their own values.
_setAppConfig({
  workersApiBase: "http://test.invalid",
  mediaWsBase: "ws://test.invalid",
  sessionLogsEnabled: false,
  sessionLogConsole: false,
  timestampedAudioEnabled: false,
  webRtcIceServers: null,
  webRtcTurnCredentialsEnabled: false,
  webCodecsIngestEnabled: false,
});

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

if (typeof globalThis.btoa === "undefined") {
  globalThis.btoa = (s: string) => Buffer.from(s, "binary").toString("base64");
}
