import { describe, it, expect } from "vitest";

import {
  InvalidWavError,
  probeWavFromBase64,
  validateVoiceSample,
  VOICE_SAMPLE_MAX_SECONDS,
  VOICE_SAMPLE_MIN_SECONDS,
  VOICE_SAMPLE_SHORT_WARN_SECONDS,
} from "../src/shared/audio/wav-duration";

// Compact WAV builder — enough control to drive every branch of the probe.
type WavFixture = {
  sampleRate?: number;
  channels?: number;
  bitsPerSample?: number;
  dataSizeBytes?: number;
  seconds?: number;
  omitData?: boolean;
  omitFmt?: boolean;
  fmtSize?: number;
  // Stuff an unknown chunk before the data chunk so the cursor-advance
  // branch has coverage.
  addJunkChunk?: boolean;
  // Deliberately corrupt header bytes so the RIFF/WAVE check fails.
  corrupt?: "riff" | "wave" | null;
};

function buildWav(f: WavFixture = {}): Uint8Array {
  const sampleRate = f.sampleRate ?? 4000;
  const channels = f.channels ?? 1;
  const bitsPerSample = f.bitsPerSample ?? 8;
  const bytesPerSample = bitsPerSample / 8;
  const dataSize =
    f.dataSizeBytes ??
    Math.round((f.seconds ?? 32) * sampleRate * channels * bytesPerSample);

  const fmtSize = f.fmtSize ?? 16;
  const junkBytes = f.addJunkChunk ? 8 + 4 : 0; // 8-byte header + 4-byte body
  const total =
    12 + (f.omitFmt ? 0 : 8 + fmtSize) + junkBytes + (f.omitData ? 0 : 8 + dataSize);
  const buf = new Uint8Array(total);
  const view = new DataView(buf.buffer);
  let cursor = 0;
  const ascii = (s: string) => {
    for (let i = 0; i < s.length; i++) buf[cursor + i] = s.charCodeAt(i);
    cursor += s.length;
  };
  const u32 = (n: number) => {
    view.setUint32(cursor, n, true);
    cursor += 4;
  };
  const u16 = (n: number) => {
    view.setUint16(cursor, n, true);
    cursor += 2;
  };

  ascii(f.corrupt === "riff" ? "XXXX" : "RIFF");
  u32(total - 8);
  ascii(f.corrupt === "wave" ? "YYYY" : "WAVE");

  if (!f.omitFmt) {
    ascii("fmt ");
    u32(fmtSize);
    if (fmtSize >= 16) {
      u16(1); // PCM
      u16(channels);
      u32(sampleRate);
      u32(sampleRate * channels * bytesPerSample);
      u16(channels * bytesPerSample);
      u16(bitsPerSample);
      // Pad out the rest of fmt (if larger than 16).
      cursor += Math.max(0, fmtSize - 16);
    } else {
      // Undersized fmt — just pad.
      cursor += fmtSize;
    }
  }

  if (f.addJunkChunk) {
    ascii("junk");
    u32(4);
    cursor += 4;
  }

  if (!f.omitData) {
    ascii("data");
    u32(dataSize);
    // data stays zero-filled.
  }
  return buf;
}

function toBase64(bytes: Uint8Array): string {
  let s = "";
  for (let i = 0; i < bytes.length; i++) s += String.fromCharCode(bytes[i]!);
  return btoa(s);
}

describe("probeWavFromBase64", () => {
  it("reads sample rate, channels, bits, duration from a well-formed WAV", () => {
    const bytes = buildWav({ seconds: 30 });
    const probe = probeWavFromBase64(toBase64(bytes));
    expect(probe.sampleRate).toBe(4000);
    expect(probe.channels).toBe(1);
    expect(probe.bitsPerSample).toBe(8);
    expect(probe.durationSeconds).toBeCloseTo(30, 5);
  });

  it("skips unknown chunks on the way to `data` (edge)", () => {
    const bytes = buildWav({ seconds: 32, addJunkChunk: true });
    const probe = probeWavFromBase64(toBase64(bytes));
    expect(probe.durationSeconds).toBeCloseTo(32, 5);
  });

  it("throws InvalidWavError when payload is shorter than a header", () => {
    expect(() => probeWavFromBase64(btoa("abc"))).toThrow(InvalidWavError);
  });

  it("throws when RIFF tag is missing (sad)", () => {
    const bytes = buildWav({ corrupt: "riff" });
    expect(() => probeWavFromBase64(toBase64(bytes))).toThrow(/RIFF/);
  });

  it("throws when WAVE tag is missing (sad)", () => {
    const bytes = buildWav({ corrupt: "wave" });
    expect(() => probeWavFromBase64(toBase64(bytes))).toThrow(/RIFF/);
  });

  it("throws when fmt chunk is smaller than 16 bytes (sad)", () => {
    const bytes = buildWav({ fmtSize: 8 });
    expect(() => probeWavFromBase64(toBase64(bytes))).toThrow(/fmt chunk too small/);
  });

  it("throws when sample rate or channels are zero (sad)", () => {
    const bytes = buildWav({ sampleRate: 0 });
    expect(() => probeWavFromBase64(toBase64(bytes))).toThrow(/invalid format chunk/);
  });

  it("throws when the data chunk is absent (sad)", () => {
    // junk padding keeps the buffer past the 44-byte RIFF header floor so
    // the walker actually runs out of chunks instead of short-circuiting.
    const bytes = buildWav({ omitData: true, addJunkChunk: true });
    expect(() => probeWavFromBase64(toBase64(bytes))).toThrow(/missing fmt or data/);
  });

  it("throws when the fmt chunk is absent (sad)", () => {
    const bytes = buildWav({ omitFmt: true });
    expect(() => probeWavFromBase64(toBase64(bytes))).toThrow(/missing fmt or data/);
  });
});

describe("validateVoiceSample", () => {
  it("accepts a 32s sample + warns that length is below the recommendation (edge)", () => {
    const bytes = buildWav({ seconds: 32 });
    const v = validateVoiceSample(toBase64(bytes));
    expect(v.ok).toBe(true);
    if (v.ok) {
      expect(v.warn).toMatch(/recommended/);
      expect(v.durationSeconds).toBeCloseTo(32, 1);
    }
  });

  it("accepts a 150s sample without a warning (happy)", () => {
    const bytes = buildWav({ seconds: 150 });
    const v = validateVoiceSample(toBase64(bytes));
    expect(v.ok).toBe(true);
    if (v.ok) expect(v.warn).toBeNull();
  });

  it("rejects a 10s sample as too short (sad)", () => {
    const bytes = buildWav({ seconds: 10 });
    const v = validateVoiceSample(toBase64(bytes));
    expect(v.ok).toBe(false);
    if (!v.ok) expect(v.error).toMatch(/too short/);
  });

  it("rejects a 200s sample as over the 180s cap (sad)", () => {
    const bytes = buildWav({ seconds: 200 });
    const v = validateVoiceSample(toBase64(bytes));
    expect(v.ok).toBe(false);
    if (!v.ok) expect(v.error).toMatch(/too long/);
  });

  it("bubbles garbage input as 'invalid wav' (sad)", () => {
    const v = validateVoiceSample(btoa("not a wav at all"));
    expect(v.ok).toBe(false);
    if (!v.ok) expect(v.durationSeconds).toBeNull();
  });

  it("maps non-InvalidWavError throws to the generic `invalid wav` error (sad)", () => {
    // `!!!!!!!!` is not a valid base64 string — atob throws DOMException,
    // which the validator should surface as a generic error rather than rethrow.
    const v = validateVoiceSample("!!!!!!!!");
    expect(v.ok).toBe(false);
    if (!v.ok) expect(v.error).toBe("invalid wav");
  });

  it("exposes the configured thresholds for other modules (happy)", () => {
    expect(VOICE_SAMPLE_MIN_SECONDS).toBe(30);
    expect(VOICE_SAMPLE_MAX_SECONDS).toBe(180);
    expect(VOICE_SAMPLE_SHORT_WARN_SECONDS).toBe(120);
  });
});
