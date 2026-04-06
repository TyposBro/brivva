import { describe, it, expect } from "vitest";
import { renderHook, act } from "@testing-library/react";
import { useDeduplicatedErrors } from "./use-deduplicated-errors";

describe("useDeduplicatedErrors", () => {
  it("should add a new error", () => {
    const { result } = renderHook(() => useDeduplicatedErrors());

    act(() => result.current.addError("connection lost"));

    expect(result.current.errors).toHaveLength(1);
    expect(result.current.errors[0].message).toBe("connection lost");
    expect(result.current.errors[0].count).toBe(1);
  });

  it("should increment count for duplicate within 5s window", () => {
    const { result } = renderHook(() => useDeduplicatedErrors());

    act(() => {
      result.current.addError("TTS failed");
      result.current.addError("TTS failed");
      result.current.addError("TTS failed");
    });

    expect(result.current.errors).toHaveLength(1);
    expect(result.current.errors[0].count).toBe(3);
  });

  it("should keep different messages separate", () => {
    const { result } = renderHook(() => useDeduplicatedErrors());

    act(() => {
      result.current.addError("error A");
      result.current.addError("error B");
    });

    expect(result.current.errors).toHaveLength(2);
    expect(result.current.errors[0].message).toBe("error A");
    expect(result.current.errors[1].message).toBe("error B");
  });

  it("should dismiss an error by index", () => {
    const { result } = renderHook(() => useDeduplicatedErrors());

    act(() => {
      result.current.addError("error A");
      result.current.addError("error B");
    });
    act(() => result.current.dismissError(0));

    expect(result.current.errors).toHaveLength(1);
    expect(result.current.errors[0].message).toBe("error B");
  });
});
