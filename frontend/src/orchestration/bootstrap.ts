// Orchestration entry point for runtime config. This is the ONLY module
// in the frontend that reads environment variables (CLAUDE.md §7).
//
// Vite replaces `import.meta.env.VITE_*` at build time, so this runs as
// a string lookup at boot, not a live env probe.

import { _setAppConfig } from "../core/config/app-config";
import { getAuthToken } from "../features/broadcast/data/api-client";
import { configureAuth, loadPersistedUser } from "../shared/auth/auth-store";

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
	const text = String(value ?? "")
		.trim()
		.toLowerCase();
	if (["0", "false", "no", "off"].includes(text)) return false;
	return true;
}

function parseIceServers(value: unknown): RTCIceServer[] | null {
	const text = String(value ?? "").trim();
	if (!text) return null;
	try {
		const parsed = JSON.parse(text) as unknown;
		if (!Array.isArray(parsed)) return null;
		return parsed.filter((entry): entry is RTCIceServer => {
			if (typeof entry !== "object" || entry === null) return false;
			const urls = (entry as { urls?: unknown }).urls;
			return typeof urls === "string" || Array.isArray(urls);
		});
	} catch {
		return null;
	}
}

export function bootstrap(): void {
	const workersApiBase =
		import.meta.env.VITE_API_URL ??
		import.meta.env.VITE_WORKER_URL ??
		DEFAULT_API_BASE;

	const mediaHttp =
		import.meta.env.VITE_MEDIA_URL ??
		import.meta.env.VITE_WORKER_URL ??
		DEFAULT_MEDIA_WS_BASE;

	_setAppConfig({
		workersApiBase,
		mediaWsBase: toWebSocketBase(mediaHttp),
		sessionLogsEnabled: envEnabledByDefault(import.meta.env.VITE_SESSION_LOGS),
		sessionLogConsole: envFlag(import.meta.env.VITE_SESSION_LOG_CONSOLE),
		timestampedAudioEnabled: envFlag(
			import.meta.env.VITE_BRIVVA_V2_TIMESTAMPED_AUDIO,
		),
		webRtcIceServers: parseIceServers(import.meta.env.VITE_WEBRTC_ICE_SERVERS),
	});

	configureAuth({
		fetchToken: async (userId) => {
			const { token } = await getAuthToken(userId);
			return token;
		},
	});
	loadPersistedUser();
}

export function toWebSocketBase(value: string): string {
	return value.replace(/^http/, "ws");
}
