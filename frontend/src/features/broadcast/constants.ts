// ── Language ─────────────────────────────────────────────

export type Lang = { code: string; label: string; flag: string };

export const LANGS: Lang[] = [
  { code: "ko", label: "Korean", flag: "\uD83C\uDDF0\uD83C\uDDF7" },
  { code: "en", label: "English", flag: "\uD83C\uDDEC\uD83C\uDDE7" },
  { code: "ja", label: "Japanese", flag: "\uD83C\uDDEF\uD83C\uDDF5" },
  { code: "zh", label: "Chinese", flag: "\uD83C\uDDE8\uD83C\uDDF3" },
] as const;

// ── Types ────────────────────────────────────────────────

export type TranslationTier = 1 | 2 | 3 | 4;

export type TranscriptEntry = {
  id: number;
  text: string;
  translations: Record<string, string>;
};

// ── Persistence ──────────────────────────────────────────

export const STORAGE_KEY = "brivva_config";

// ── Video capture ────────────────────────────────────────

export const VIDEO_WIDTH = 1920;
export const VIDEO_HEIGHT = 1080;
export const VIDEO_FPS = 60;
export const VIDEO_BITRATE = 8_000_000;
export const RECORDING_CHUNK_MS = 100;

// ── Voice cloning ────────────────────────────────────────

export const CLONE_DURATION_SEC = 30;
export const CLONE_SAMPLE_RATE = 44100;
export const CLONE_BUFFER_SIZE = 4096;
export const CLONE_PROGRESS_INTERVAL_MS = 200;

// ── Broadcast delay ──────────────────────────────────────

export const DELAY_MIN = 1000;
export const DELAY_MAX = 10000;
export const DELAY_STEP = 500;

// ── WebSocket tag bytes ──────────────────────────────────

export const AUDIO_TAG = 0x01;
export const VIDEO_TAG = 0x02;
