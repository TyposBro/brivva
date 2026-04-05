import { LANGS } from "../../shared/platforms";

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

// Re-export for convenience
export { LANGS };
