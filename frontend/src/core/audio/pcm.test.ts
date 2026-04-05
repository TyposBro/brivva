import { describe, it, expect } from "vitest";
import { float32ToInt16, mergePcmChunks } from "./pcm";

describe("float32ToInt16", () => {
  it("should_convert_zero_to_zero", () => {
    const result = float32ToInt16(new Float32Array([0]));

    expect(result[0]).toBe(0);
  });

  it("should_clamp_positive_overflow", () => {
    const result = float32ToInt16(new Float32Array([2.0]));

    expect(result[0]).toBe(32767);
  });

  it("should_clamp_negative_overflow", () => {
    const result = float32ToInt16(new Float32Array([-2.0]));

    expect(result[0]).toBe(-32768);
  });

  it("should_scale_half_to_16384", () => {
    const result = float32ToInt16(new Float32Array([0.5]));

    expect(result[0]).toBe(16384);
  });

  it("should_clamp_exactly_one_to_max", () => {
    const result = float32ToInt16(new Float32Array([1.0]));

    expect(result[0]).toBe(32767);
  });

  it("should_handle_negative_one_as_min", () => {
    const result = float32ToInt16(new Float32Array([-1.0]));

    expect(result[0]).toBe(-32768);
  });
});

describe("mergePcmChunks", () => {
  it("should_return_empty_for_empty_array", () => {
    const result = mergePcmChunks([]);

    expect(result.length).toBe(0);
  });

  it("should_passthrough_single_chunk", () => {
    const chunk = new Int16Array([1, 2, 3]);

    const result = mergePcmChunks([chunk]);

    expect(Array.from(result)).toEqual([1, 2, 3]);
  });

  it("should_merge_multiple_chunks", () => {
    const a = new Int16Array([1, 2]);
    const b = new Int16Array([3, 4]);

    const result = mergePcmChunks([a, b]);

    expect(Array.from(result)).toEqual([1, 2, 3, 4]);
  });
});
