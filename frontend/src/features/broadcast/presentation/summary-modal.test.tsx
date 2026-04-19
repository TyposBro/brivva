import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

const fetchSessionSummary = vi.fn();
vi.mock("../data/quote-api", () => ({
  fetchSessionSummary: (...args: unknown[]) => fetchSessionSummary(...args),
}));

import { SummaryModal } from "./summary-modal";

describe("SummaryModal", () => {
  beforeEach(() => {
    fetchSessionSummary.mockReset();
  });

  it("happy: renders totals and per-lang breakdown", async () => {
    fetchSessionSummary.mockResolvedValue({
      totalMinutes: 63,
      totalCostUsd: 189,
      breakdown: [
        { lang: "ja", minutes: 63, costUsd: 94.5 },
        { lang: "zh", minutes: 63, costUsd: 94.5 },
      ],
    });
    render(<SummaryModal sessionId="s1" onClose={vi.fn()} />);
    await waitFor(() => expect(screen.getByText("63m")).toBeInTheDocument());
    expect(screen.getByText("$189.00")).toBeInTheDocument();
    expect(screen.getAllByText(/\$94\.50/)).toHaveLength(2);
  });

  it("sad: renders unavailable message when fetch rejects", async () => {
    fetchSessionSummary.mockRejectedValue(new Error("not implemented"));
    render(<SummaryModal sessionId="s1" onClose={vi.fn()} />);
    await waitFor(() =>
      expect(screen.getByText(/Summary unavailable/i)).toBeInTheDocument(),
    );
    expect(screen.getByText(/not implemented/)).toBeInTheDocument();
  });

  it("Close button invokes onClose", async () => {
    fetchSessionSummary.mockResolvedValue({
      totalMinutes: 0,
      totalCostUsd: 0,
      breakdown: [],
    });
    const onClose = vi.fn();
    render(<SummaryModal sessionId="s1" onClose={onClose} />);
    await waitFor(() => expect(screen.getByText("0m")).toBeInTheDocument());
    await userEvent.click(screen.getByRole("button", { name: /Close/i }));
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});
