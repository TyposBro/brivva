import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { DestinationCard, PlatformIcon, type Destination } from "./dashboard-destination-card";

function makeDest(over: Partial<Destination> = {}): Destination {
  return {
    uid: "u1",
    platform: "youtube",
    lang: "en",
    rtmp_url: "rtmp://a.rtmp.youtube.com/live2/",
    stream_key: "key",
    delay_ms: 2000,
    host_gain: 0.2,
    ...over,
  };
}

describe("PlatformIcon", () => {
  it("renders the matching icon per platform id", () => {
    const { container, rerender } = render(<PlatformIcon id="youtube" />);
    expect(container.querySelector("svg")).toBeInTheDocument();
    rerender(<PlatformIcon id="twitch" />);
    expect(container.querySelector("svg")).toBeInTheDocument();
    rerender(<PlatformIcon id="local-test" />);
    expect(container.querySelector("svg")).toBeInTheDocument();
    rerender(<PlatformIcon id="something-else" />);
    expect(container.querySelector("svg")).toBeInTheDocument();
  });
});

describe("DestinationCard", () => {
  it("renders auto platform header + Advanced toggle (happy)", async () => {
    // ko→en stream-default is 3000ms / 0.2, so this dest should start
    // collapsed since values match the curated defaults.
    const dest = makeDest({ platform: "youtube", lang: "en", delay_ms: 3000 });
    render(
      <DestinationCard
        dest={dest}
        sourceLang="ko"
        savedCreds={{}}
        user={null}
        userId="u-test"
        privacyStatus="unlisted"
        onPrivacyChange={vi.fn()}
        onUpdate={vi.fn()}
        onRemove={vi.fn()}
      />,
    );
    expect(screen.getByText(/YouTube/i)).toBeInTheDocument();
    const advanced = screen.getByRole("button", { name: /Advanced/i });
    expect(screen.queryByText(/Output delay/i)).not.toBeInTheDocument();
    await userEvent.click(advanced);
    expect(screen.getByText(/Output delay/i)).toBeInTheDocument();
  });

  it("auto-expands Advanced when defaults differ from the curated table (sad)", () => {
    const dest = makeDest({ platform: "custom", lang: "ja", delay_ms: 4321, host_gain: 0.5 });
    render(
      <DestinationCard
        dest={dest}
        sourceLang="ko"
        savedCreds={{}}
        user={null}
        userId="u-test"
        privacyStatus="unlisted"
        onPrivacyChange={vi.fn()}
        onUpdate={vi.fn()}
        onRemove={vi.fn()}
      />,
    );
    // Override detected → sliders visible from the start.
    expect(screen.getByText(/Output delay/i)).toBeInTheDocument();
    expect(screen.getByText("4321 ms")).toBeInTheDocument();
  });

  it("language select fires onUpdate when target lang changes (happy)", async () => {
    const dest = makeDest({ platform: "custom", lang: "ja" });
    const onUpdate = vi.fn();
    render(
      <DestinationCard
        dest={dest}
        sourceLang="ko"
        savedCreds={{}}
        user={null}
        userId="u-test"
        privacyStatus="unlisted"
        onPrivacyChange={vi.fn()}
        onUpdate={onUpdate}
        onRemove={vi.fn()}
      />,
    );
    await userEvent.selectOptions(screen.getByRole("combobox"), "zh");
    expect(onUpdate).toHaveBeenCalledWith({ lang: "zh" });
  });

  it("X button fires onRemove (sad)", async () => {
    const onRemove = vi.fn();
    const { container } = render(
      <DestinationCard
        dest={makeDest({ platform: "youtube", lang: "en", delay_ms: 3000 })}
        sourceLang="ko"
        savedCreds={{}}
        user={null}
        userId="u-test"
        privacyStatus="unlisted"
        onPrivacyChange={vi.fn()}
        onUpdate={vi.fn()}
        onRemove={onRemove}
      />,
    );
    // Header X button is the only button styled with `hover:text-error`.
    const removeBtn = container.querySelector("button.hover\\:text-error");
    expect(removeBtn).not.toBeNull();
    await userEvent.click(removeBtn as HTMLButtonElement);
    expect(onRemove).toHaveBeenCalled();
  });

  it("selecting Passthrough fires onUpdate with lang=pass (happy)", async () => {
    const dest = makeDest({ platform: "custom", lang: "ja" });
    const onUpdate = vi.fn();
    render(
      <DestinationCard
        dest={dest}
        sourceLang="ko"
        savedCreds={{}}
        user={null}
        userId="u-test"
        privacyStatus="unlisted"
        onPrivacyChange={vi.fn()}
        onUpdate={onUpdate}
        onRemove={vi.fn()}
      />,
    );
    await userEvent.selectOptions(screen.getByRole("combobox"), "pass");
    expect(onUpdate).toHaveBeenCalledWith({ lang: "pass" });
  });

  it("passthrough destination renders raw pill and suppresses timing sliders (happy)", async () => {
    const dest = makeDest({ platform: "custom", lang: "pass", delay_ms: 0, host_gain: 1.0 });
    render(
      <DestinationCard
        dest={dest}
        sourceLang="ko"
        savedCreds={{}}
        user={null}
        userId="u-test"
        privacyStatus="unlisted"
        onPrivacyChange={vi.fn()}
        onUpdate={vi.fn()}
        onRemove={vi.fn()}
      />,
    );
    // Raw badge sits next to the lang select as a visual cue.
    expect(screen.getByText(/^raw$/i)).toBeInTheDocument();
    // Expand Advanced and verify it shows explanatory copy, not the delay
    // slider a translated destination would have.
    await userEvent.click(screen.getByRole("button", { name: /Advanced/i }));
    expect(screen.queryByText(/Output delay/i)).not.toBeInTheDocument();
    expect(
      screen.getByText(/re-broadcast the host's raw audio/i),
    ).toBeInTheDocument();
  });

  it("returns null for unknown platform (sad)", () => {
    const { container } = render(
      <DestinationCard
        dest={makeDest({ platform: "no-such-thing" })}
        sourceLang="ko"
        savedCreds={{}}
        user={null}
        userId="u-test"
        privacyStatus="unlisted"
        onPrivacyChange={vi.fn()}
        onUpdate={vi.fn()}
        onRemove={vi.fn()}
      />,
    );
    expect(container.firstChild).toBeNull();
  });
});
