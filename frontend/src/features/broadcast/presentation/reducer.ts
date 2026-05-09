import type {
  MediaIngestMode,
  MediaServerCapabilities,
  ResolvedMediaIngestMode,
} from "./media-ingest-mode";
import type { WebCodecsVideoStats } from "./use-webcodecs-video";

export type HostStatus =
  | "idle"
  | "creating"
  | "voice_setup"
  | "cloning"
  | "ready"
  | "recording"
  | "disconnected";

export type HostUtterance = { id: number; transcript: string };

export interface MediaDiagnostics {
  mediaIngest?: {
    requested: MediaIngestMode;
    resolved: ResolvedMediaIngestMode;
    active?: ResolvedMediaIngestMode;
    codec?: string;
  };
  source?: { width?: number; height?: number; frameRate?: number };
  outbound?: {
    frameWidth?: number;
    frameHeight?: number;
    framesPerSecond?: number;
    framesSent?: number;
    qualityLimitationReason?: unknown;
    codec?: unknown;
    candidatePair?: unknown;
  };
  webcodecs?: WebCodecsVideoStats;
}

export interface ProviderHealthNotice {
  provider: string;
  state: string;
  recoverable: boolean;
  billable: boolean;
  reason: string;
  targetLang?: string;
  statusCode?: number;
  errorCode?: string;
  message: string;
}

export interface HostState {
  status: HostStatus;
  liveTranscript: string;
  utterances: HostUtterance[];
  translations: Record<string, { id: number; text: string }>; // keyed by target lang
  analyser: AnalyserNode | null;
  error: string | null;
  voiceReady: boolean;
  mediaDiagnostics: MediaDiagnostics | null;
  mediaServerCapabilities: MediaServerCapabilities | null;
  activeMediaIngestMode: ResolvedMediaIngestMode | null;
  connectionIssue: string | null;
  providerHealth: ProviderHealthNotice[];
}

export type HostAction =
  | { type: "reset" }
  | { type: "connected" }
  | { type: "interim"; transcript: string }
  | { type: "final"; id: number; transcript: string }
  | { type: "translation"; id: number; targetLang: string; text: string }
  | { type: "error"; message: string }
  | {
      type: "recording_started";
      analyser: AnalyserNode;
      requestedMediaIngestMode: MediaIngestMode;
      mediaIngestMode: ResolvedMediaIngestMode;
    }
  | { type: "recording_stopped" }
  | { type: "disconnected" }
  | { type: "voice_cloning" }
  | { type: "voice_ready" }
  | { type: "skip_voice_setup" }
  | { type: "media_diagnostics"; diagnostics: MediaDiagnostics }
  | { type: "server_capabilities"; capabilities: MediaServerCapabilities }
  | { type: "connection_issue"; message: string }
  | { type: "provider_health"; notice: ProviderHealthNotice };

export const INITIAL_STATE: HostState = {
  status: "idle",
  liveTranscript: "",
  utterances: [],
  translations: {},
  analyser: null,
  error: null,
  voiceReady: false,
  mediaDiagnostics: null,
  mediaServerCapabilities: null,
  activeMediaIngestMode: null,
  connectionIssue: null,
  providerHealth: [],
};

export function hostReducer(state: HostState, action: HostAction): HostState {
  switch (action.type) {
    case "reset":
      return { ...INITIAL_STATE, status: "creating" };

    case "connected":
      return { ...state, status: "voice_setup" };

    case "interim":
      return { ...state, liveTranscript: action.transcript };

    case "final":
      return {
        ...state,
        liveTranscript: "",
        utterances: [
          ...state.utterances,
          { id: action.id, transcript: action.transcript },
        ],
      };

    case "translation":
      return {
        ...state,
        translations: {
          ...state.translations,
          [action.targetLang]: { id: action.id, text: action.text },
        },
      };

    case "error":
      return { ...state, error: action.message };

    case "recording_started":
      return {
        ...state,
        status: "recording",
        analyser: action.analyser,
        activeMediaIngestMode: action.mediaIngestMode,
        mediaDiagnostics: {
          ...state.mediaDiagnostics,
          mediaIngest: {
            requested: action.requestedMediaIngestMode,
            resolved: action.mediaIngestMode,
            active: action.mediaIngestMode,
          },
        },
        connectionIssue: null,
      };

    case "recording_stopped":
      return {
        ...state,
        status: state.status === "recording" ? "ready" : state.status,
        analyser: null,
      };

    case "disconnected":
      return { ...state, status: "disconnected", analyser: null };

    case "voice_cloning":
      return { ...state, status: "cloning" };

    case "voice_ready":
      return { ...state, status: "ready", voiceReady: true };

    case "skip_voice_setup":
      return { ...state, status: "ready" };

    case "media_diagnostics":
      return {
        ...state,
        mediaDiagnostics: { ...state.mediaDiagnostics, ...action.diagnostics },
      };

    case "server_capabilities":
      return { ...state, mediaServerCapabilities: action.capabilities };

    case "connection_issue":
      return { ...state, connectionIssue: action.message };

    case "provider_health": {
      const key = (notice: ProviderHealthNotice) =>
        `${notice.provider}:${notice.targetLang ?? ""}:${notice.reason}`;
      const next = state.providerHealth.filter(
        (notice) => key(notice) !== key(action.notice),
      );
      return { ...state, providerHealth: [...next, action.notice].slice(-10) };
    }
  }
}
