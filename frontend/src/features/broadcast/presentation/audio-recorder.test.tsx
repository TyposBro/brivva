import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { AudioRecorder } from "./audio-recorder";

class FakeAnalyser {
  fftSize = 512;
  frequencyBinCount = 256;
  getByteFrequencyData(buf: Uint8Array) {
    for (let i = 0; i < buf.length; i++) buf[i] = 128;
  }
  getByteTimeDomainData(buf: Uint8Array) {
    for (let i = 0; i < buf.length; i++) buf[i] = 128;
  }
}

beforeEach(() => {
  // jsdom canvas is a no-op; provide a getContext stub that returns the
  // bare 2D methods AudioRecorder calls during the draw loop.
  HTMLCanvasElement.prototype.getContext = vi.fn(() => ({
    fillStyle: "",
    fillRect: vi.fn(),
    strokeStyle: "",
    lineWidth: 0,
    beginPath: vi.fn(),
    moveTo: vi.fn(),
    lineTo: vi.fn(),
    stroke: vi.fn(),
  })) as unknown as typeof HTMLCanvasElement.prototype.getContext;
  vi.stubGlobal("requestAnimationFrame", vi.fn(() => 1));
  vi.stubGlobal("cancelAnimationFrame", vi.fn());
});

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("AudioRecorder", () => {
  it("idle state shows Start button (happy)", async () => {
    const onStart = vi.fn();
    render(
      <AudioRecorder
        isRecording={false}
        analyser={null}
        onStart={onStart}
        onStop={vi.fn()}
      />,
    );
    const btn = screen.getByRole("button");
    await userEvent.click(btn);
    expect(onStart).toHaveBeenCalled();
  });

  it("recording state with analyser starts the draw loop + Stop button", async () => {
    const onStop = vi.fn();
    render(
      <AudioRecorder
        isRecording={true}
        analyser={new FakeAnalyser() as unknown as AnalyserNode}
        onStart={vi.fn()}
        onStop={onStop}
      />,
    );
    expect(requestAnimationFrame).toHaveBeenCalled();
    await userEvent.click(screen.getByRole("button"));
    expect(onStop).toHaveBeenCalled();
  });
});
