// Base64 WAV → duration in seconds. Used by the voice-clone upload path to
// reject samples that are too short for ElevenLabs to produce a usable clone
// (30s floor), warn on short-but-accepted samples (< 2min), and cap the
// clone length at 3min so a stray hour-long file can't tie up the worker.
//
// Pure: takes base64, returns a plain object. No fetch, no Env. Lives in
// shared/ so it can sit next to shared/auth/ without the layer checker
// thinking it's a feature.

export type WavProbe = {
  durationSeconds: number;
  sampleRate: number;
  channels: number;
  bitsPerSample: number;
  dataSizeBytes: number;
};

export class InvalidWavError extends Error {}

// Decode base64 to bytes without pulling in node:buffer (Workers runtime).
function base64ToBytes(b64: string): Uint8Array {
  const binary = atob(b64);
  const out = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) out[i] = binary.charCodeAt(i);
  return out;
}

function readUint32LE(buf: Uint8Array, offset: number): number {
  return (
    buf[offset]! |
    (buf[offset + 1]! << 8) |
    (buf[offset + 2]! << 16) |
    (buf[offset + 3]! << 24)
  ) >>> 0;
}

function readUint16LE(buf: Uint8Array, offset: number): number {
  return (buf[offset]! | (buf[offset + 1]! << 8)) & 0xffff;
}

function readAscii(buf: Uint8Array, offset: number, length: number): string {
  let s = "";
  for (let i = 0; i < length; i++) s += String.fromCharCode(buf[offset + i]!);
  return s;
}

/**
 * Parses a minimal WAV container (RIFF/WAVE, PCM or IEEE float) and returns
 * duration + format. Throws `InvalidWavError` if the header is not a WAV we
 * can read — the caller should treat that as a 400.
 */
export function probeWavFromBase64(audioBase64: string): WavProbe {
  const bytes = base64ToBytes(audioBase64);
  if (bytes.length < 44) {
    throw new InvalidWavError("audio payload too small to be a WAV file");
  }

  if (readAscii(bytes, 0, 4) !== "RIFF" || readAscii(bytes, 8, 4) !== "WAVE") {
    throw new InvalidWavError("not a RIFF/WAVE container");
  }

  // Walk chunks after the `WAVE` tag. `fmt ` gives sample rate + channels,
  // `data` gives byte count for duration. Skip chunks we don't care about.
  let cursor = 12;
  let sampleRate = 0;
  let channels = 0;
  let bitsPerSample = 0;
  let dataSize = 0;
  let sawFmt = false;
  let sawData = false;

  while (cursor + 8 <= bytes.length) {
    const id = readAscii(bytes, cursor, 4);
    const size = readUint32LE(bytes, cursor + 4);
    const body = cursor + 8;

    if (id === "fmt ") {
      if (size < 16) throw new InvalidWavError("fmt chunk too small");
      channels = readUint16LE(bytes, body + 2);
      sampleRate = readUint32LE(bytes, body + 4);
      bitsPerSample = readUint16LE(bytes, body + 14);
      sawFmt = true;
    } else if (id === "data") {
      dataSize = size;
      sawData = true;
      break;
    }

    // Chunk bodies are padded to an even byte count.
    cursor = body + size + (size % 2);
  }

  if (!sawFmt || !sawData) {
    throw new InvalidWavError("missing fmt or data chunk");
  }
  if (sampleRate === 0 || channels === 0 || bitsPerSample === 0) {
    throw new InvalidWavError("invalid format chunk");
  }

  const bytesPerSample = bitsPerSample / 8;
  const durationSeconds = dataSize / (sampleRate * channels * bytesPerSample);
  return {
    durationSeconds,
    sampleRate,
    channels,
    bitsPerSample,
    dataSizeBytes: dataSize,
  };
}

export const VOICE_SAMPLE_MIN_SECONDS = 30;
export const VOICE_SAMPLE_MAX_SECONDS = 180;
export const VOICE_SAMPLE_SHORT_WARN_SECONDS = 120;

export type VoiceSampleValidation =
  | { ok: true; durationSeconds: number; warn: string | null }
  | { ok: false; error: string; durationSeconds: number | null };

export function validateVoiceSample(audioBase64: string): VoiceSampleValidation {
  let probe: WavProbe;
  try {
    probe = probeWavFromBase64(audioBase64);
  } catch (e) {
    return {
      ok: false,
      error: e instanceof InvalidWavError ? e.message : "invalid wav",
      durationSeconds: null,
    };
  }

  const d = probe.durationSeconds;
  if (d < VOICE_SAMPLE_MIN_SECONDS) {
    return {
      ok: false,
      error: `voice sample too short: ${d.toFixed(1)}s < ${VOICE_SAMPLE_MIN_SECONDS}s minimum`,
      durationSeconds: d,
    };
  }
  if (d > VOICE_SAMPLE_MAX_SECONDS) {
    return {
      ok: false,
      error: `voice sample too long: ${d.toFixed(1)}s > ${VOICE_SAMPLE_MAX_SECONDS}s cap`,
      durationSeconds: d,
    };
  }
  const warn =
    d < VOICE_SAMPLE_SHORT_WARN_SECONDS
      ? `voice sample is ${d.toFixed(1)}s; ${VOICE_SAMPLE_SHORT_WARN_SECONDS}s+ recommended for best clone fidelity`
      : null;
  return { ok: true, durationSeconds: d, warn };
}
