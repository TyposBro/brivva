// Orchestration entry point for runtime config. This is the ONLY module
// in the frontend that reads environment variables (CLAUDE.md §7).
//
// Vite replaces `import.meta.env.VITE_*` at build time, so this runs as
// a string lookup at boot, not a live env probe.

import { _setAppConfig } from "../core/config/app-config";
import { configureAuth, loadPersistedUser } from "../shared/auth/auth-store";
import { getAuthToken } from "../features/broadcast/data/api-client";

const DEFAULT_API_BASE = "http://localhost:3000";
const DEFAULT_MEDIA_WS_BASE = "http://localhost:8787";

function envFlag(value: unknown): boolean {
  return ["1", "true", "yes", "on"].includes(
    String(value ?? "")
      .trim()
      .toLowerCase(),
  );
}

function envEnabledByDefault(value: unknown): boolean {
  const text = String(value ?? "").trim().toLowerCase();
  if (["0", "false", "no", "off"].includes(text)) return false;
  return true;
}

export function bootstrap(): void {
  const workersApiBase =
    import.meta.env.VITE_API_URL ??
    import.meta.env.VITE_WORKER_URL ??
    DEFAULT_API_BASE;

  const mediaHttp = import.meta.env.VITE_WORKER_URL ?? DEFAULT_MEDIA_WS_BASE;

  _setAppConfig({
    workersApiBase,
    mediaWsBase: mediaHttp.replace(/^http/, "ws"),
    sessionLogsEnabled: envEnabledByDefault(import.meta.env.VITE_SESSION_LOGS),
    sessionLogConsole: envFlag(import.meta.env.VITE_SESSION_LOG_CONSOLE),
  });

  configureAuth({
    fetchToken: async (userId) => {
      const { token } = await getAuthToken(userId);
      return token;
    },
  });
  loadPersistedUser();
}
