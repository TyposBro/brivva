// --- domain types ---

export type Lang = "en" | "ja" | "zh";
export const LANGS: Lang[] = ["en", "ja", "zh"];

// --- service interfaces (used by usecases) ---

export type SttCallbacks = {
  onInterim: (transcript: string) => void;
  onFinal: (transcript: string, utteranceId: number) => void;
};

export type Stt = {
  connect(sourceLang: string): Promise<void>;
  sendAudio(data: ArrayBuffer): void;
  close(): void;
};

export type SttFactory = (roomId: string, callbacks: SttCallbacks) => Stt;

export type TranslationResult = {
  lang: Lang;
  text: string;
  translateMs: number;
};

export type Translator = {
  translateAll(transcript: string, langs: Lang[]): Promise<TranslationResult[]>;
};

export type TranslatorFactory = (sourceLang: string, roomId: string) => Translator;

export type Tts = {
  synthesize(lang: Lang, text: string): Promise<ReadableStream | null>;
};

export type TtsFactory = (roomId: string) => Tts;

export type HostBroadcaster = {
  hasHost: boolean;
  setHost(ws: WebSocket): void;
  clearHost(): void;
  activeLangs(): Lang[];
  sendToHost(data: string): void;
  sendToLang(lang: Lang, data: string | Uint8Array): void;
  sendToEveryone(data: string): void;
  sendToAllGuests(data: string): void;
};

export type GuestBroadcaster = {
  hasHost: boolean;
  addGuest(id: string, ws: WebSocket, lang: Lang): void;
  removeGuest(id: string): void;
  pushGuestCount(): void;
};
