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
    // ko→en stream-default is 1000ms / 0.03 (retuned 2026-04-22), so this
    // dest should start collapsed since values match the curated defaults.
    const dest = makeDest({ platform: "youtube", lang: "en", delay_ms: 1000, host_gain: 0.03 });
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

  // ── Ephemeral key platforms (Grip) ──
  // Grip stream keys are one-shot per broadcast (AWS IVS rejects a duplicate
  // publisher, causing silent mid-stream failure). The destination card must
  // render paste inputs + an amber warning, but NEVER a Save button and
  // NEVER a "Pre-filled" badge — reusing a stale saved key is exactly the
  // broken state we're preventing.

  it("Grip renders the one-shot-key warning block + paste inputs (happy)", () => {
    const dest = makeDest({
      platform: "grip",
      lang: "zh",
      rtmp_url: "",
      stream_key: "",
      delay_ms: 2500,
      host_gain: 0.2,
    });
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
    // Warning block visible + worded around one-shot keys.
    const warning = screen.getByTestId("grip-ephemeral-warning");
    expect(warning).toBeInTheDocument();
    expect(warning.textContent).toMatch(/one-shot/i);
    expect(warning.textContent).toMatch(/fresh/i);
    // Paste inputs still render so the host can enter the key.
    expect(screen.getByPlaceholderText(/Server URL/i)).toBeInTheDocument();
    expect(screen.getByPlaceholderText(/Stream Key/i)).toBeInTheDocument();
  });

  it("Grip does NOT render a Save credentials button (sad)", () => {
    const dest = makeDest({
      platform: "grip",
      lang: "zh",
      rtmp_url: "rtmps://live-a.example/app",
      stream_key: "grip-key-one-shot",
    });
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
    expect(
      screen.queryByRole("button", { name: /Save credentials/i }),
    ).not.toBeInTheDocument();
  });

  it("Grip does NOT render the 'Pre-filled' badge even when savedCreds has a grip row (sad)", () => {
    // Defence-in-depth: the dashboard-page filter strips Grip rows out of
    // savedCreds before they reach the card, but if a future refactor
    // regresses that guard, the card itself must still refuse to show the
    // Pre-filled badge — otherwise a host would reuse a stale one-shot key
    // and silently fail the second session.
    const dest = makeDest({
      platform: "grip",
      lang: "zh",
      rtmp_url: "rtmps://live-a.example/app",
      stream_key: "grip-key-one-shot",
    });
    render(
      <DestinationCard
        dest={dest}
        sourceLang="ko"
        savedCreds={{
          grip: {
            id: "pc-grip",
            user_id: "u-test",
            platform: "grip",
            rtmp_url: "rtmps://stale.example/app",
            stream_key: "stale-key",
            display_name: null,
            created_at: 1,
            updated_at: 1,
          },
        }}
        user={null}
        userId="u-test"
        privacyStatus="unlisted"
        onPrivacyChange={vi.fn()}
        onUpdate={vi.fn()}
        onRemove={vi.fn()}
      />,
    );
    expect(
      screen.queryByText(/Pre-filled from saved credentials/i),
    ).not.toBeInTheDocument();
  });

  it("TikTok still renders Save credentials — regression guard (happy)", () => {
    // TikTok stream keys are reusable until the host regenerates them; the
    // save path was NOT removed. This test pins that the Grip change didn't
    // accidentally strip Save from every paste-creds platform.
    const dest = makeDest({
      platform: "tiktok",
      lang: "en",
      rtmp_url: "rtmps://push.tiktokcdn.com/game/",
      stream_key: "tiktok-reusable-key",
    });
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
    expect(
      screen.getByRole("button", { name: /Save credentials/i }),
    ).toBeInTheDocument();
    expect(
      screen.queryByTestId("grip-ephemeral-warning"),
    ).not.toBeInTheDocument();
  });
});
