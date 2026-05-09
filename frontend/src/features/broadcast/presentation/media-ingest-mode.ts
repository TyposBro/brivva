import { appConfig } from "../../../core/config/app-config";

export type MediaIngestMode = "auto" | "webrtc" | "webcodecs_ws";
export type ResolvedMediaIngestMode = "webrtc" | "webcodecs_ws";

export type MediaServerCapabilities = {
  videoIngestModes: ResolvedMediaIngestMode[];
  webcodecsCodecs: string[];
};

export type WebCodecsSupport = {
  available: boolean;
  reasons: string[];
};

export const MEDIA_INGEST_MODE_STORAGE_KEY = "brivva:mediaIngestMode";

const VALID_MEDIA_INGEST_MODES = new Set<MediaIngestMode>([
  "auto",
  "webrtc",
  "webcodecs_ws",
]);

export function isMediaIngestMode(value: unknown): value is MediaIngestMode {
  return typeof value === "string" && VALID_MEDIA_INGEST_MODES.has(value as MediaIngestMode);
}

export function readStoredMediaIngestMode(): MediaIngestMode {
  try {
    const raw = globalThis.localStorage?.getItem(MEDIA_INGEST_MODE_STORAGE_KEY);
    return isMediaIngestMode(raw) ? raw : "auto";
  } catch {
    return "auto";
  }
}

export function writeStoredMediaIngestMode(mode: MediaIngestMode): void {
  try {
    globalThis.localStorage?.setItem(MEDIA_INGEST_MODE_STORAGE_KEY, mode);
  } catch {
    // Private-mode/SSR-safe: this is an operator convenience only.
  }
}

export function resolveMediaIngestMode(
  requested: MediaIngestMode,
): ResolvedMediaIngestMode {
  if (requested === "webrtc") return "webrtc";
  if (requested === "webcodecs_ws") return "webcodecs_ws";
  return "webrtc";
}

export function mediaIngestModeLabel(mode: MediaIngestMode | ResolvedMediaIngestMode): string {
  switch (mode) {
    case "auto":
      return "Auto";
    case "webrtc":
      return "WebRTC hardened";
    case "webcodecs_ws":
      return "WebCodecs over WebSocket";
  }
}

export function detectWebCodecsSupport(): WebCodecsSupport {
  const reasons: string[] = [];
  const runtime = globalThis as unknown as Record<string, unknown>;
  if (typeof runtime.VideoEncoder === "undefined") reasons.push("VideoEncoder");
  if (typeof runtime.VideoFrame === "undefined") reasons.push("VideoFrame");
  if (typeof runtime.MediaStreamTrackProcessor === "undefined") {
    reasons.push("MediaStreamTrackProcessor");
  }
  return { available: reasons.length === 0, reasons };
}

export function webCodecsFrontendEnabled(): boolean {
  return appConfig().webCodecsIngestEnabled;
}

export function webCodecsRecordBlocker(
  requested: MediaIngestMode,
  serverCapabilities: MediaServerCapabilities | null,
): string | null {
  if (resolveMediaIngestMode(requested) !== "webcodecs_ws") return null;
  if (!webCodecsFrontendEnabled()) {
    return "WebCodecs ingest is disabled in this frontend build. Use WebRTC or enable VITE_WEBCODECS_INGEST_ENABLED.";
  }
  const support = detectWebCodecsSupport();
  if (!support.available) {
    return "WebCodecs ingest is not available in this browser. Use WebRTC or switch to Chrome/Brave.";
  }
  if (!serverCapabilities) {
    return "Checking media server WebCodecs capability…";
  }
  if (!serverCapabilities.videoIngestModes.includes("webcodecs_ws")) {
    return "WebCodecs ingest is disabled on this server. Use WebRTC or enable BRIVVA_WEBCODECS_INGEST_ENABLED.";
  }
  if (!serverCapabilities.webcodecsCodecs.includes("vp8")) {
    return "WebCodecs VP8 ingest is not available on this server. Use WebRTC.";
  }
  return null;
}

export function parseServerCapabilities(msg: unknown): MediaServerCapabilities | null {
  if (typeof msg !== "object" || msg === null) return null;
  const typed = msg as {
    type?: unknown;
    videoIngestModes?: unknown;
    webcodecsCodecs?: unknown;
  };
  if (typed.type !== "server:capabilities") return null;
  const videoIngestModes = Array.isArray(typed.videoIngestModes)
    ? typed.videoIngestModes.filter((mode): mode is ResolvedMediaIngestMode =>
        mode === "webrtc" || mode === "webcodecs_ws",
      )
    : [];
  const webcodecsCodecs = Array.isArray(typed.webcodecsCodecs)
    ? typed.webcodecsCodecs.filter((codec): codec is string => typeof codec === "string")
    : [];
  return { videoIngestModes, webcodecsCodecs };
}
