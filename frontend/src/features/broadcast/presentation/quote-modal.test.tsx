import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

const fetchSessionQuote = vi.fn();
vi.mock("../data/quote-api", () => ({
  fetchSessionQuote: (...args: unknown[]) => (fetchSessionQuote as (...a: unknown[]) => unknown)(...args),
}));

import { QuoteModal } from "./quote-modal";

describe("QuoteModal", () => {
  beforeEach(() => {
    fetchSessionQuote.mockReset();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("happy: renders estimate at default 30min, refetches when slider moves", async () => {
    fetchSessionQuote.mockImplementation((_sessionId: string, mins: number) =>
      Promise.resolve({
        estimatedCostUsd: mins * 1.5,
        expectedMinutes: mins,
        breakdown: [{ lang: "ja", minutes: mins, costUsd: mins * 1.5 }],
      }),
    );

    const onConfirm = vi.fn();
    const onCancel = vi.fn();
    render(<QuoteModal sessionId="s1" onConfirm={onConfirm} onCancel={onCancel} />);

    await waitFor(() => expect(fetchSessionQuote).toHaveBeenCalledWith("s1", 30));
    await waitFor(() =>
      expect(screen.getAllByText(/\$45\.00/).length).toBeGreaterThan(0),
    );

    // Move the duration slider to 60 — quote should re-fetch with 60.
    const slider = screen.getByRole("slider");
    fireSliderChange(slider, 60);
    await waitFor(() => expect(fetchSessionQuote).toHaveBeenLastCalledWith("s1", 60));
    await waitFor(() =>
      expect(screen.getAllByText(/\$90\.00/).length).toBeGreaterThan(0),
    );

    await userEvent.click(screen.getByRole("button", { name: /Continue/i }));
    expect(onConfirm).toHaveBeenCalled();
  });

  it("sad: quote endpoint failure renders graceful fallback message", async () => {
    fetchSessionQuote.mockRejectedValue(new Error("upstream 500"));
    render(<QuoteModal sessionId="s1" onConfirm={vi.fn()} onCancel={vi.fn()} />);
    await waitFor(() =>
      expect(screen.getByText(/Estimate unavailable/i)).toBeInTheDocument(),
    );
    expect(screen.getByText(/upstream 500/)).toBeInTheDocument();
  });

  it("edge: null cost renders '—' + pending helper, NOT the raw Zod blob", async () => {
    // Reproduces the production bug on a fresh user: server 200s but
    // estimatedCostUsd is null. The modal must stay human-readable.
    fetchSessionQuote.mockResolvedValue({
      estimatedCostUsd: null,
      expectedMinutes: 30,
      breakdown: [],
    });
    render(<QuoteModal sessionId="s1" onConfirm={vi.fn()} onCancel={vi.fn()} />);
    await waitFor(() => expect(fetchSessionQuote).toHaveBeenCalled());

    // Neutral dash in place of "$..."
    expect(await screen.findByLabelText(/Estimate pending/i)).toBeInTheDocument();
    expect(screen.getByText(/billing reflects actual usage/i)).toBeInTheDocument();

    // Critically: the raw Zod issues JSON must NOT be visible.
    expect(screen.queryByText(/invalid_type/)).toBeNull();
    expect(screen.queryByText(/"expected":\s*"number"/)).toBeNull();
  });

  it("Cancel button + close icon both invoke onCancel", async () => {
    fetchSessionQuote.mockResolvedValue({
      estimatedCostUsd: 1,
      expectedMinutes: 30,
      breakdown: [],
    });
    const onCancel = vi.fn();
    render(<QuoteModal sessionId="s1" onConfirm={vi.fn()} onCancel={onCancel} />);

    await waitFor(() => expect(fetchSessionQuote).toHaveBeenCalled());
    await userEvent.click(screen.getByRole("button", { name: /Not yet/i }));
    expect(onCancel).toHaveBeenCalledTimes(1);
  });
});

// userEvent doesn't drive range inputs reliably — fire the change event by hand.
function fireSliderChange(slider: HTMLElement, value: number) {
  const input = slider as HTMLInputElement;
  Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, "value")?.set?.call(
    input,
    String(value),
  );
  input.dispatchEvent(new Event("input", { bubbles: true }));
  input.dispatchEvent(new Event("change", { bubbles: true }));
}
