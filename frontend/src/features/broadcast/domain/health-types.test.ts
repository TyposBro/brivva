import { describe, it, expect } from "vitest";
import { classifyStreamStatus, computeRollingAverage } from "./health-types";

describe("classifyStreamStatus", () => {
  it("should return green when drift and queue depth are low", () => {
    expect(classifyStreamStatus(200, 2)).toBe("green");
  });

  it("should return yellow when drift exceeds 500ms", () => {
    expect(classifyStreamStatus(600, 2)).toBe("yellow");
  });

  it("should return yellow when queue depth exceeds 3", () => {
    expect(classifyStreamStatus(200, 5)).toBe("yellow");
  });

  it("should return red when drift exceeds 2000ms", () => {
    expect(classifyStreamStatus(2500, 1)).toBe("red");
  });

  it("should return red when queue depth exceeds 7", () => {
    expect(classifyStreamStatus(100, 9)).toBe("red");
  });

  it("should return green at exact boundary of 500ms drift", () => {
    expect(classifyStreamStatus(500, 3)).toBe("green");
  });

  it("should return yellow at exact boundary of 2000ms drift", () => {
    expect(classifyStreamStatus(2000, 7)).toBe("yellow");
  });

  it("should prioritize red over yellow when both thresholds met", () => {
    expect(classifyStreamStatus(2500, 5)).toBe("red");
  });
});

describe("computeRollingAverage", () => {
  it("should compute average of a single sample", () => {
    const result = computeRollingAverage([], 100);
    expect(result.average).toBe(100);
    expect(result.samples).toEqual([100]);
  });

  it("should compute average of multiple samples", () => {
    const result = computeRollingAverage([100, 200], 300);
    expect(result.average).toBe(200);
    expect(result.samples).toEqual([100, 200, 300]);
  });

  it("should evict oldest sample when window is full", () => {
    const samples = Array.from({ length: 12 }, (_, i) => i * 10);
    const result = computeRollingAverage(samples, 999);
    expect(result.samples).toHaveLength(12);
    expect(result.samples[0]).toBe(10);
    expect(result.samples[11]).toBe(999);
  });
});
