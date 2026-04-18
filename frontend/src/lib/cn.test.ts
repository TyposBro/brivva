import { describe, it, expect } from "vitest";
import { cn } from "./cn";

describe("cn", () => {
  it("joins string classes", () => {
    expect(cn("a", "b", "c")).toBe("a b c");
  });

  it("ignores falsy values", () => {
    expect(cn("a", false, null, undefined, "", "b")).toBe("a b");
  });

  it("conditional object syntax", () => {
    expect(cn("base", { active: true, hidden: false })).toBe("base active");
  });

  it("flattens nested arrays", () => {
    expect(cn(["a", ["b", ["c"]]])).toBe("a b c");
  });

  it("tailwind conflict merge — last wins for padding", () => {
    expect(cn("p-2", "p-4")).toBe("p-4");
  });

  it("tailwind conflict — text colors", () => {
    expect(cn("text-red-500", "text-blue-500")).toBe("text-blue-500");
  });

  it("non-conflicting classes preserved", () => {
    expect(cn("px-2", "py-4", "bg-red-500")).toContain("bg-red-500");
  });

  it("empty input → empty string", () => {
    expect(cn()).toBe("");
  });
});
