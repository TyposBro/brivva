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
  /** HTTP URL for the Fargate media server (http:// or https://). */
  mediaHttpBase: string;
  /** Host video ingest path. JPEG is the compatibility fallback. */
  videoIngest: "jpeg" | "webrtc";
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
