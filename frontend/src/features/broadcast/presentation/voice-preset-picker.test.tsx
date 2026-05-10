import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { VoicePresetPicker } from "./voice-preset-picker";

describe("VoicePresetPicker", () => {
  it("shows built-in Yuna and Gitae instant-clone presets", async () => {
    const onChange = vi.fn();

    render(
      <VoicePresetPicker
        value="female"
        onChange={onChange}
        hasClone={false}
        onReRecord={vi.fn()}
      />,
    );

    expect(screen.getByRole("button", { name: /Yuna/i })).toBeInTheDocument();
    expect(screen.getAllByText(/Instant clone, every language/i)).toHaveLength(2);
    expect(screen.getByRole("button", { name: /Gitae/i })).toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: /Yuna/i }));
    expect(onChange).toHaveBeenCalledWith("yuna");
  });
});
