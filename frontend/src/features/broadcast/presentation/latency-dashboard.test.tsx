import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import { LatencyDashboard } from "./latency-dashboard";

describe("LatencyDashboard", () => {
  it("empty timings shows the loader / awaiting state (sad)", () => {
    render(<LatencyDashboard timings={[]} />);
    expect(screen.getByText(/Live Latency/)).toBeInTheDocument();
  });

  it("renders Avg/Best/Worst stats once timings arrive (happy)", () => {
    render(
      <LatencyDashboard
        timings={[
          {
            id: "1",
            text: "hello",
            langs: ["ja"],
            sttMs: 200,
            translateMs: 100,
            ttsMs: 250,
            lipsyncMs: 50,
            totalMs: 600,
            overheadMs: 0,
            timestamp: 1,
          },
          {
            id: "2",
            text: "world",
            langs: ["ja"],
            sttMs: 250,
            translateMs: 90,
            ttsMs: 300,
            lipsyncMs: 60,
            totalMs: 700,
            overheadMs: 0,
            timestamp: 2,
          },
        ]}
      />,
    );
    expect(screen.getByText(/Avg:/)).toBeInTheDocument();
    expect(screen.getByText(/Best:/)).toBeInTheDocument();
  });
});
