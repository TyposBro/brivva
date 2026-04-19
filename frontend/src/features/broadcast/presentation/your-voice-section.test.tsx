import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

const recorder = {
  start: vi.fn(),
  stop: vi.fn(),
  elapsedSec: 0,
  isRecording: false,
};

vi.mock("../../../shared/audio/voice-recorder", () => ({
  useVoiceRecorder: vi.fn(() => recorder),
}));

const createVoice = vi.fn();
vi.mock("../data/api-client", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../data/api-client")>();
  return {
    ...actual,
    createVoice: (...args: unknown[]) => createVoice(...args),
  };
});

import { YourVoiceSection } from "./your-voice-section";

describe("YourVoiceSection", () => {
  beforeEach(() => {
    recorder.start.mockReset();
    recorder.stop.mockReset();
    recorder.elapsedSec = 0;
    recorder.isRecording = false;
    createVoice.mockReset();
  });

  it("renders 'Record your voice' when no voice exists (sad: empty)", () => {
    render(
      <YourVoiceSection
        userId="u1"
        voice={null}
        defaultName="Aziz"
        onChange={vi.fn()}
      />,
    );
    expect(screen.getByRole("button", { name: /Record your voice/i })).toBeInTheDocument();
  });

  it("renders existing voice with Re-record (happy)", async () => {
    render(
      <YourVoiceSection
        userId="u1"
        voice={{
          id: "v1",
          user_id: "u1",
          elevenlabs_voice_id: "el1",
          name: "Aziz Voice",
          source_lang: "en",
          created_at: 1,
        }}
        defaultName="fallback"
        onChange={vi.fn()}
      />,
    );
    expect(screen.getByText("Aziz Voice")).toBeInTheDocument();
    const re = screen.getByRole("button", { name: /Re-record/i });
    await userEvent.click(re);
    expect(screen.getByRole("button", { name: /Record Voice Sample/i })).toBeInTheDocument();
  });

  it("upload happy: stop returns base64 → createVoice → onChange fires", async () => {
    recorder.stop.mockReturnValue("BASE64");
    recorder.elapsedSec = 45;
    recorder.isRecording = true;
    createVoice.mockResolvedValue({
      id: "v2",
      user_id: "u1",
      elevenlabs_voice_id: "el2",
      name: "fresh",
      source_lang: "en",
      created_at: 99,
    });
    const onChange = vi.fn();
    render(
      <YourVoiceSection
        userId="u1"
        voice={null}
        defaultName="fresh"
        onChange={onChange}
      />,
    );
    await userEvent.click(screen.getByRole("button", { name: /Record your voice/i }));

    const stopBtn = await screen.findByRole("button", { name: /Stop & Clone/i });
    await userEvent.click(stopBtn);

    await waitFor(() => expect(createVoice).toHaveBeenCalledWith(
      expect.objectContaining({ user_id: "u1", audio_base64: "BASE64", name: "fresh" }),
    ));
    // source_lang must ride the payload — onboarding used to be the only
    // place that set it; dashboard now sends it too.
    expect(createVoice).toHaveBeenCalledWith(
      expect.objectContaining({ source_lang: expect.any(String) }),
    );
    await waitFor(() => expect(onChange).toHaveBeenCalled());
  });

  it("re-record: defaults source-lang from voice.source_lang, user can override", async () => {
    recorder.stop.mockReturnValue("BASE64");
    recorder.elapsedSec = 45;
    recorder.isRecording = true;
    createVoice.mockResolvedValue({
      id: "v3",
      user_id: "u1",
      elevenlabs_voice_id: "el3",
      name: "Aziz",
      source_lang: "ja",
      created_at: 10,
    });
    render(
      <YourVoiceSection
        userId="u1"
        voice={{
          id: "v1",
          user_id: "u1",
          elevenlabs_voice_id: "el1",
          name: "Aziz",
          source_lang: "ko",
          created_at: 1,
        }}
        defaultName="Aziz"
        onChange={vi.fn()}
      />,
    );
    await userEvent.click(screen.getByRole("button", { name: /Re-record/i }));

    // Seeded from voice.source_lang = "ko".
    expect(screen.getByRole("radio", { name: /Korean/i })).toHaveAttribute(
      "aria-checked",
      "true",
    );

    // Host switches to Japanese before stopping.
    await userEvent.click(screen.getByRole("radio", { name: /Japanese/i }));

    const stopBtn = screen.getByRole("button", { name: /Stop & Clone/i });
    await userEvent.click(stopBtn);

    await waitFor(() =>
      expect(createVoice).toHaveBeenCalledWith(
        expect.objectContaining({ source_lang: "ja" }),
      ),
    );
  });

  it("upload sad: surfaces error message when API rejects", async () => {
    recorder.stop.mockReturnValue("BASE64");
    recorder.elapsedSec = 60;
    recorder.isRecording = true;
    createVoice.mockRejectedValue(new Error("upload blew up"));
    render(
      <YourVoiceSection
        userId="u1"
        voice={null}
        defaultName="x"
        onChange={vi.fn()}
      />,
    );
    await userEvent.click(screen.getByRole("button", { name: /Record your voice/i }));
    const stopBtn = await screen.findByRole("button", { name: /Stop & Clone/i });
    await userEvent.click(stopBtn);
    await waitFor(() =>
      expect(screen.getByText(/upload blew up/)).toBeInTheDocument(),
    );
  });
});
