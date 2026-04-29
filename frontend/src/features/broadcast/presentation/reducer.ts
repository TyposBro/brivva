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
  source?: { width?: number; height?: number; frameRate?: number };
  outbound?: { frameWidth?: number; frameHeight?: number; framesPerSecond?: number; framesSent?: number; qualityLimitationReason?: unknown };
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
  connectionIssue: string | null;
}

export type HostAction =
  | { type: "reset" }
  | { type: "connected" }
  | { type: "interim"; transcript: string }
  | { type: "final"; id: number; transcript: string }
  | { type: "translation"; id: number; targetLang: string; text: string }
  | { type: "error"; message: string }
  | { type: "recording_started"; analyser: AnalyserNode }
  | { type: "recording_stopped" }
  | { type: "disconnected" }
  | { type: "voice_cloning" }
  | { type: "voice_ready" }
  | { type: "skip_voice_setup" }
  | { type: "media_diagnostics"; diagnostics: MediaDiagnostics }
  | { type: "connection_issue"; message: string };

export const INITIAL_STATE: HostState = {
  status: "idle",
  liveTranscript: "",
  utterances: [],
  translations: {},
  analyser: null,
  error: null,
  voiceReady: false,
  mediaDiagnostics: null,
  connectionIssue: null,
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
      return { ...state, status: "recording", analyser: action.analyser, connectionIssue: null };

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
      return { ...state, mediaDiagnostics: action.diagnostics };

    case "connection_issue":
      return { ...state, connectionIssue: action.message };
  }
}
