import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { VoiceSetupCard } from "./voice-setup-card";

const baseProps = {
  minSec: 30,
  maxSec: 180,
  onStart: vi.fn(),
  onStop: vi.fn(),
  onSkip: vi.fn(),
};

describe("VoiceSetupCard", () => {
  it("idle state shows 'Record Voice Sample' button (happy)", async () => {
    const onStart = vi.fn();
    render(
      <VoiceSetupCard
        {...baseProps}
        elapsedSec={0}
        isRecording={false}
        onStart={onStart}
      />,
    );
    const btn = screen.getByRole("button", { name: /Record Voice Sample/i });
    await userEvent.click(btn);
    expect(onStart).toHaveBeenCalled();
  });

  it("recording, below minimum: stop is disabled with countdown label", () => {
    render(
      <VoiceSetupCard
        {...baseProps}
        elapsedSec={15}
        isRecording={true}
      />,
    );
    expect(screen.getByText(/00:15 \/ 00:30/)).toBeInTheDocument();
    const stop = screen.getByRole("button", { name: /15s to minimum/i });
    expect(stop).toBeDisabled();
  });

  it("recording, at-or-above minimum: stop is enabled, label flips to max (happy)", async () => {
    const onStop = vi.fn();
    render(
      <VoiceSetupCard
        {...baseProps}
        elapsedSec={45}
        isRecording={true}
        onStop={onStop}
      />,
    );
    expect(screen.getByText(/00:45 \/ 03:00 max/)).toBeInTheDocument();
    const stop = screen.getByRole("button", { name: /Stop & Clone/i });
    expect(stop).not.toBeDisabled();
    await userEvent.click(stop);
    expect(onStop).toHaveBeenCalled();
  });

  it("Skip button always available (sad: bypass voice clone)", async () => {
    const onSkip = vi.fn();
    render(
      <VoiceSetupCard
        {...baseProps}
        elapsedSec={0}
        isRecording={false}
        onSkip={onSkip}
      />,
    );
    await userEvent.click(screen.getByRole("button", { name: /Skip/i }));
    expect(onSkip).toHaveBeenCalled();
  });
});
