// Runtime-accessible config for the frontend bundle.
//
// CLAUDE.md §7: env is read ONLY in orchestration. This file exposes the
// resulting values via a lazy singleton — lower layers call `appConfig()`,
// they never reach for `import.meta.env`.
//
// `setAppConfig` is intentionally prefixed with `_` to discourage calls
// from outside orchestration; the one legitimate caller is
// `orchestration/bootstrap.ts`.

export interface AppConfig {
  /** Base URL of the Workers API (no trailing slash). */
  workersApiBase: string;
  /** WebSocket URL for the Fargate media server (ws:// or wss://). */
  mediaWsBase: string;
  /** D1-backed session log upload toggle. */
  sessionLogsEnabled: boolean;
  /** Mirror structured session logs to browser console. */
  sessionLogConsole: boolean;
  /** V2 Phase 2 timestamped WS PCM bridge. Default off for deploy safety. */
  timestampedAudioEnabled: boolean;
  /** Optional JSON-encoded RTCIceServer[] for TURN/STUN overrides. */
  webRtcIceServers: RTCIceServer[] | null;
}

let current: AppConfig | null = null;

export function _setAppConfig(next: AppConfig): void {
  current = next;
}

export function appConfig(): AppConfig {
  if (!current) {
    throw new Error(
      "AppConfig not initialised. orchestration/bootstrap.ts must run before any feature code imports app-config.",
    );
  }
  return current;
}
