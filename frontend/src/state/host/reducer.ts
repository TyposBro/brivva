export type HostStatus = "idle" | "creating" | "ready" | "recording" | "disconnected";
export type GuestCounts = { en: number; ja: number; zh: number };
export type HostUtterance = { id: number; transcript: string };

export interface HostState {
  status: HostStatus;
  roomId: string | null;
  guestCounts: GuestCounts;
  liveTranscript: string;
  utterances: HostUtterance[];
  analyser: AnalyserNode | null;
  error: string | null;
}

export type HostAction =
  | { type: "reset" }
  | { type: "room_created"; roomId: string }
  | { type: "guest_count"; counts: GuestCounts }
  | { type: "interim"; transcript: string }
  | { type: "final"; id: number; transcript: string }
  | { type: "error"; message: string }
  | { type: "recording_started"; analyser: AnalyserNode }
  | { type: "recording_stopped" }
  | { type: "disconnected" };

const EMPTY_COUNTS: GuestCounts = { en: 0, ja: 0, zh: 0 };

export const INITIAL_STATE: HostState = {
  status: "idle",
  roomId: null,
  guestCounts: EMPTY_COUNTS,
  liveTranscript: "",
  utterances: [],
  analyser: null,
  error: null,
};

export function hostReducer(state: HostState, action: HostAction): HostState {
  switch (action.type) {
    case "reset":
      return { ...INITIAL_STATE, status: "creating" };

    case "room_created":
      return { ...state, status: "ready", roomId: action.roomId };

    case "guest_count":
      return { ...state, guestCounts: action.counts };

    case "interim":
      return { ...state, liveTranscript: action.transcript };

    case "final":
      return {
        ...state,
        liveTranscript: "",
        utterances: [...state.utterances, { id: action.id, transcript: action.transcript }],
      };

    case "error":
      return { ...state, error: action.message };

    case "recording_started":
      return { ...state, status: "recording", analyser: action.analyser };

    case "recording_stopped":
      return {
        ...state,
        status: state.status === "recording" ? "ready" : state.status,
        analyser: null,
      };

    case "disconnected":
      return { ...state, status: "disconnected", analyser: null };
  }
}
