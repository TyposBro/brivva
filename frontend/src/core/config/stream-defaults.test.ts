import { describe, it, expect } from "vitest";
import { isStreamDefault, streamDefault } from "./stream-defaults";

describe("streamDefault", () => {
  it("ko→zh uses curated 3000ms / 20% mix (happy)", () => {
    expect(streamDefault("ko", "zh")).toEqual({ delay_ms: 3000, host_gain: 0.2 });
  });

  it("ko→ja and ko→en match ko→zh — Korean source needs the longer hold (happy)", () => {
    expect(streamDefault("ko", "ja")).toEqual({ delay_ms: 3000, host_gain: 0.2 });
    expect(streamDefault("ko", "en")).toEqual({ delay_ms: 3000, host_gain: 0.2 });
  });

  it("source-equal-target is passthrough (edge)", () => {
    expect(streamDefault("ko", "ko")).toEqual({ delay_ms: 0, host_gain: 1.0 });
    expect(streamDefault("en", "en")).toEqual({ delay_ms: 0, host_gain: 1.0 });
  });

  it("unknown pair falls back to baseline 2000/20% (sad)", () => {
    expect(streamDefault("uk", "ru")).toEqual({ delay_ms: 2000, host_gain: 0.2 });
  });
});

describe("isStreamDefault", () => {
  it("recognises matching curated value (happy)", () => {
    expect(isStreamDefault("ko", "zh", { delay_ms: 3000, host_gain: 0.2 })).toBe(true);
  });

  it("flags any user override (happy)", () => {
    expect(isStreamDefault("ko", "zh", { delay_ms: 2500, host_gain: 0.2 })).toBe(false);
    expect(isStreamDefault("ko", "zh", { delay_ms: 3000, host_gain: 0.5 })).toBe(false);
  });
});
