import { describe, it, expect } from "vitest";
import { isStreamDefault, streamDefault } from "./stream-defaults";

describe("streamDefault", () => {
  it("ko→zh uses curated 1000ms / 3% mix (happy)", () => {
    expect(streamDefault("ko", "zh")).toEqual({ delay_ms: 1000, host_gain: 0.03 });
  });

  it("ko→ja and ko→en match ko→zh — Korean source needs the longer hold (happy)", () => {
    expect(streamDefault("ko", "ja")).toEqual({ delay_ms: 1000, host_gain: 0.03 });
    expect(streamDefault("ko", "en")).toEqual({ delay_ms: 1000, host_gain: 0.03 });
  });

  it("source-equal-target is passthrough (edge)", () => {
    expect(streamDefault("ko", "ko")).toEqual({ delay_ms: 0, host_gain: 1.0 });
    expect(streamDefault("en", "en")).toEqual({ delay_ms: 0, host_gain: 1.0 });
  });

  it("unknown pair falls back to baseline 750/3% (sad)", () => {
    expect(streamDefault("uk", "ru")).toEqual({ delay_ms: 750, host_gain: 0.03 });
  });

  it("ko→SEA (th/vi/id) uses curated 750/3% baseline (happy)", () => {
    expect(streamDefault("ko", "th")).toEqual({ delay_ms: 750, host_gain: 0.03 });
    expect(streamDefault("ko", "vi")).toEqual({ delay_ms: 750, host_gain: 0.03 });
    expect(streamDefault("ko", "id")).toEqual({ delay_ms: 750, host_gain: 0.03 });
  });
});

describe("isStreamDefault", () => {
  it("recognises matching curated value (happy)", () => {
    expect(isStreamDefault("ko", "zh", { delay_ms: 1000, host_gain: 0.03 })).toBe(true);
  });

  it("flags any user override (happy)", () => {
    expect(isStreamDefault("ko", "zh", { delay_ms: 750, host_gain: 0.03 })).toBe(false);
    expect(isStreamDefault("ko", "zh", { delay_ms: 1000, host_gain: 0.5 })).toBe(false);
  });
});
