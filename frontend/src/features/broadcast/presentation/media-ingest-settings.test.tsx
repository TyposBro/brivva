import { describe, expect, it, beforeEach, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { _setAppConfig } from "../../../core/config/app-config";
import {
  AdvancedMediaIngestSettings,
  useMediaIngestModePreference,
} from "./media-ingest-settings";
import { MEDIA_INGEST_MODE_STORAGE_KEY } from "./media-ingest-mode";

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

function Harness() {
  const [mode, setMode] = useMediaIngestModePreference();
  return <AdvancedMediaIngestSettings value={mode} onChange={setMode} />;
}

describe("AdvancedMediaIngestSettings", () => {
  beforeEach(() => {
    vi.unstubAllGlobals();
    vi.stubGlobal("localStorage", new FakeStorage());
    setConfig(false);
  });

  it("renders copy and disables WebCodecs when feature/browser support is missing", () => {
    render(<Harness />);
    expect(screen.getByText("Advanced media settings")).toBeInTheDocument();
    expect(screen.getByLabelText(/Auto \(recommended\)/i)).toBeChecked();
    expect(screen.getByLabelText(/WebCodecs over WebSocket/i)).toBeDisabled();
    expect(screen.getByText(/Unavailable in this browser/i)).toBeInTheDocument();
  });

  it("persists operator selection to localStorage", async () => {
    setConfig(true);
    vi.stubGlobal("VideoEncoder", class {});
    vi.stubGlobal("VideoFrame", class {});
    vi.stubGlobal("MediaStreamTrackProcessor", class {});
    render(<Harness />);

    await userEvent.click(screen.getByLabelText(/WebRTC hardened/i));

    expect(localStorage.getItem(MEDIA_INGEST_MODE_STORAGE_KEY)).toBe("webrtc");
  });
});
