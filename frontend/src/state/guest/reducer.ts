export type GuestStatus = "idle" | "connecting" | "listening" | "closed" | "error";
export type GuestUtterance = {
  id: number;
  original: string;
  translation: string;
};

export interface GuestState {
  status: GuestStatus;
  liveTranscript: string;
  utterances: GuestUtterance[];
  error: string | null;
}

export type GuestAction =
  | { type: "reset" }
  | { type: "joined" }
  | { type: "interim"; transcript: string }
  | { type: "final"; id: number; original: string }
  | { type: "translation"; id: number; text: string }
  | { type: "closed" }
  | { type: "error"; message: string };

export const INITIAL_STATE: GuestState = {
  status: "idle",
  liveTranscript: "",
  utterances: [],
  error: null,
};

export function guestReducer(state: GuestState, action: GuestAction): GuestState {
  switch (action.type) {
    case "reset":
      return { ...INITIAL_STATE, status: "connecting" };

    case "joined":
      return { ...state, status: "listening" };

    case "interim":
      return { ...state, liveTranscript: action.transcript };

    case "final":
      return {
        ...state,
        liveTranscript: "",
        utterances: [...state.utterances, { id: action.id, original: action.original, translation: "" }],
      };

    case "translation":
      return {
        ...state,
        utterances: state.utterances.map((u) =>
          u.id === action.id ? { ...u, translation: action.text } : u,
        ),
      };

    case "closed":
      return { ...state, status: "closed" };

    case "error":
      return { ...state, status: "error", error: action.message };
  }
}
