import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { act, renderHook } from "@testing-library/react";
import { useTimings } from "./useTimings";

describe("useTimings", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-01-01T00:00:00Z"));
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("starts empty", () => {
    const { result } = renderHook(() => useTimings());
    expect(result.current.timings).toEqual([]);
  });

  it("happy path: interim → final → translate → tts_end finalizes with total", () => {
    const { result } = renderHook(() => useTimings());
    act(() => result.current.markInterim());
    vi.advanceTimersByTime(200);
    act(() => result.current.startTimer("u1", "hello world", ["ja"]));
    act(() => result.current.recordTranslate("u1", 150));
    vi.advanceTimersByTime(400);
    act(() => result.current.recordTts("u1", 300));

    expect(result.current.timings).toHaveLength(1);
    const t = result.current.timings[0];
    expect(t.id).toBe("u1");
    expect(t.text).toBe("hello world");
    expect(t.sttMs).toBe(200);
    expect(t.translateMs).toBe(150);
    expect(t.ttsMs).toBe(300);
    expect(t.lipsyncMs).toBe(0);
    expect(t.langs).toEqual(["ja"]);
    expect(t.totalMs).toBeGreaterThanOrEqual(500);
    expect(t.overheadMs).toBeGreaterThanOrEqual(0);
  });

  it("startTimer without markInterim → sttMs=0 (edge: no interim seen)", () => {
    const { result } = renderHook(() => useTimings());
    act(() => result.current.startTimer("u1", "t", ["ja"]));
    act(() => result.current.recordTts("u1", 50));
    expect(result.current.timings[0].sttMs).toBe(0);
  });

  it("markInterim is idempotent within same utterance", () => {
    const { result } = renderHook(() => useTimings());
    act(() => result.current.markInterim());
    vi.advanceTimersByTime(100);
    act(() => result.current.markInterim());
    vi.advanceTimersByTime(100);
    act(() => result.current.startTimer("u1", "x", []));
    act(() => result.current.recordTts("u1", 1));
    expect(result.current.timings[0].sttMs).toBe(200);
  });

  it("text truncated to 40 chars", () => {
    const { result } = renderHook(() => useTimings());
    const long = "a".repeat(100);
    act(() => result.current.startTimer("u1", long, []));
    act(() => result.current.recordTts("u1", 1));
    expect(result.current.timings[0].text).toHaveLength(40);
  });

  it("recordStt guarded: only sets when entry has no sttMs yet", () => {
    const { result } = renderHook(() => useTimings());
    act(() => result.current.markInterim());
    vi.advanceTimersByTime(100);
    act(() => result.current.startTimer("u1", "x", []));
    act(() => result.current.recordStt("u1", 999));
    act(() => result.current.recordTts("u1", 1));
    expect(result.current.timings[0].sttMs).toBe(100);
  });

  it("recordTranslate guarded against double-record", () => {
    const { result } = renderHook(() => useTimings());
    act(() => result.current.startTimer("u1", "x", []));
    act(() => result.current.recordTranslate("u1", 50));
    act(() => result.current.recordTranslate("u1", 999));
    act(() => result.current.recordTts("u1", 1));
    expect(result.current.timings[0].translateMs).toBe(50);
  });

  it("recordTts on unknown uid → no-op (sad)", () => {
    const { result } = renderHook(() => useTimings());
    act(() => result.current.recordTts("ghost", 100));
    expect(result.current.timings).toEqual([]);
  });

  it("ring buffer caps at 10; newest first", () => {
    const { result } = renderHook(() => useTimings());
    for (let i = 0; i < 15; i++) {
      act(() => result.current.startTimer(`u${i}`, `t${i}`, []));
      act(() => result.current.recordTts(`u${i}`, 1));
    }
    expect(result.current.timings).toHaveLength(10);
    expect(result.current.timings[0].id).toBe("u14");
    expect(result.current.timings[9].id).toBe("u5");
  });

  it("reset clears timings + pending + interim state", () => {
    const { result } = renderHook(() => useTimings());
    act(() => result.current.markInterim());
    act(() => result.current.startTimer("u1", "x", []));
    act(() => result.current.recordTts("u1", 1));
    expect(result.current.timings).toHaveLength(1);
    act(() => result.current.reset());
    expect(result.current.timings).toEqual([]);
    // After reset, new interim should start fresh
    vi.advanceTimersByTime(100);
    act(() => result.current.markInterim());
    vi.advanceTimersByTime(50);
    act(() => result.current.startTimer("u2", "y", []));
    act(() => result.current.recordTts("u2", 1));
    expect(result.current.timings[0].sttMs).toBe(50);
  });

  it("finalize is a no-op (compat shim)", () => {
    const { result } = renderHook(() => useTimings());
    act(() => result.current.startTimer("u1", "x", []));
    act(() => result.current.finalize("u1", 999));
    act(() => result.current.recordTts("u1", 1));
    expect(result.current.timings[0].lipsyncMs).toBe(0);
  });
});
