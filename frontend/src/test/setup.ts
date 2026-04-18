import "@testing-library/jest-dom/vitest";
import { afterEach, vi } from "vitest";
import { cleanup } from "@testing-library/react";

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

if (typeof globalThis.btoa === "undefined") {
  globalThis.btoa = (s: string) => Buffer.from(s, "binary").toString("base64");
}
