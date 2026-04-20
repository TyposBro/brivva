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

  it("happy (self_serve): renders totals, price, and per-lang breakdown", async () => {
    fetchSessionSummary.mockResolvedValue({
      billingTier: "self_serve",
      sourceMinutes: 63,
      totalMinutes: 63,
      totalCostUsd: 189,
      rateUsd: 1.5,
      billedTo: null,
      breakdown: [
        { lang: "ja", minutes: 63, costUsd: 94.5 },
        { lang: "zh", minutes: 63, costUsd: 94.5 },
      ],
    });
    render(<SummaryModal sessionId="s1" onClose={vi.fn()} />);
    await waitFor(() => expect(screen.getByText("63m")).toBeInTheDocument());
    expect(screen.getByText("$189.00")).toBeInTheDocument();
    expect(screen.getAllByText(/\$94\.50/)).toHaveLength(2);
    // Self-serve must NOT render a "Billed to" label.
    expect(screen.queryByText(/Billed to/i)).toBeNull();
  });

  it("happy (b2b): shows 'Billed to Simon' and NO dollar figure anywhere", async () => {
    fetchSessionSummary.mockResolvedValue({
      billingTier: "b2b",
      sourceMinutes: 10,
      totalMinutes: 10,
      totalCostUsd: null,
      rateUsd: null,
      billedTo: "Simon",
      breakdown: [{ lang: "ja", minutes: 10, costUsd: 0 }],
    });
    render(<SummaryModal sessionId="s1" onClose={vi.fn()} />);
    // Both the aggregate header and the per-lang row render "10m" on b2b.
    await waitFor(() => expect(screen.getAllByText("10m")).toHaveLength(2));
    expect(screen.getByText(/Billed to/i)).toBeInTheDocument();
    expect(screen.getByText("Simon")).toBeInTheDocument();
    // Critical invariant: no "$" anywhere in the rendered modal for B2B.
    expect(document.body.textContent ?? "").not.toMatch(/\$/);
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
      billingTier: "self_serve",
      sourceMinutes: 0,
      totalMinutes: 0,
      totalCostUsd: 0,
      rateUsd: 1.5,
      billedTo: null,
      breakdown: [],
    });
    const onClose = vi.fn();
    render(<SummaryModal sessionId="s1" onClose={onClose} />);
    await waitFor(() => expect(screen.getByText("0m")).toBeInTheDocument());
    await userEvent.click(screen.getByRole("button", { name: /Close/i }));
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});
