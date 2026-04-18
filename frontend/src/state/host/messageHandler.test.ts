import { describe, it, expect, vi, beforeEach } from "vitest";
import { createMessageHandler } from "./messageHandler";
import type { HostAction } from "./reducer";

function makeHarness(langs: string[] = ["ja", "zh"]) {
  const dispatch = vi.fn<(a: HostAction) => void>();
  const stopwatch = {
    startTimer: vi.fn(),
    markInterim: vi.fn(),
    recordStt: vi.fn(),
    recordTranslate: vi.fn(),
    recordTts: vi.fn(),
    finalize: vi.fn(),
  };
  const handler = createMessageHandler(dispatch, () => langs, stopwatch);
  return { dispatch, stopwatch, handler };
}

describe("createMessageHandler", () => {
  let h: ReturnType<typeof makeHarness>;
  beforeEach(() => { h = makeHarness(); });

  it("interim → dispatch + markInterim", () => {
    h.handler({ type: "interim", transcript: "hel" });
    expect(h.dispatch).toHaveBeenCalledWith({ type: "interim", transcript: "hel" });
    expect(h.stopwatch.markInterim).toHaveBeenCalledOnce();
  });

  it("interim missing transcript → defaults empty string", () => {
    h.handler({ type: "interim" });
    expect(h.dispatch).toHaveBeenCalledWith({ type: "interim", transcript: "" });
  });

  it("final → dispatch + startTimer w/ active langs", () => {
    h.handler({ type: "final", utteranceId: 7, transcript: "done" });
    expect(h.dispatch).toHaveBeenCalledWith({ type: "final", id: 7, transcript: "done" });
    expect(h.stopwatch.startTimer).toHaveBeenCalledWith("7", "done", ["ja", "zh"]);
  });

  it("final w/ sttMs → recordStt called", () => {
    h.handler({ type: "final", utteranceId: 7, transcript: "d", sttMs: 250 });
    expect(h.stopwatch.recordStt).toHaveBeenCalledWith("7", 250);
  });

  it("final w/o sttMs → recordStt not called", () => {
    h.handler({ type: "final", utteranceId: 7, transcript: "d" });
    expect(h.stopwatch.recordStt).not.toHaveBeenCalled();
  });

  it("translation → dispatch + recordTranslate", () => {
    h.handler({
      type: "translation", utteranceId: 1, targetLang: "ja", text: "こ", translateMs: 120,
    });
    expect(h.dispatch).toHaveBeenCalledWith({
      type: "translation", id: 1, targetLang: "ja", text: "こ",
    });
    expect(h.stopwatch.recordTranslate).toHaveBeenCalledWith("1", 120);
  });

  it("translation w/o translateMs → recordTranslate skipped", () => {
    h.handler({ type: "translation", utteranceId: 1, targetLang: "ja", text: "x" });
    expect(h.stopwatch.recordTranslate).not.toHaveBeenCalled();
  });

  it("tts_end → recordTts", () => {
    h.handler({ type: "tts_end", utteranceId: 1, ttsMs: 300 });
    expect(h.stopwatch.recordTts).toHaveBeenCalledWith("1", 300);
  });

  it("tts_end w/o ttsMs → skipped (sad)", () => {
    h.handler({ type: "tts_end", utteranceId: 1 });
    expect(h.stopwatch.recordTts).not.toHaveBeenCalled();
  });

  it("video_end → finalize", () => {
    h.handler({ type: "video_end", utteranceId: 1, lipsyncMs: 80 });
    expect(h.stopwatch.finalize).toHaveBeenCalledWith("1", 80);
  });

  it("video_end w/o lipsyncMs → 0 default", () => {
    h.handler({ type: "video_end", utteranceId: 1 });
    expect(h.stopwatch.finalize).toHaveBeenCalledWith("1", 0);
  });

  it("voice:ready → dispatch voice_ready", () => {
    h.handler({ type: "voice:ready", voiceId: "v1" });
    expect(h.dispatch).toHaveBeenCalledWith({ type: "voice_ready" });
  });

  it("error → dispatch w/ message", () => {
    h.handler({ type: "error", message: "boom" });
    expect(h.dispatch).toHaveBeenCalledWith({ type: "error", message: "boom" });
  });

  it("error w/o message → 'Unknown error' fallback", () => {
    h.handler({ type: "error" });
    expect(h.dispatch).toHaveBeenCalledWith({ type: "error", message: "Unknown error" });
  });

  it("unknown msg.type → no dispatch, no stopwatch calls (sad)", () => {
    h.handler({ type: "mystery" });
    expect(h.dispatch).not.toHaveBeenCalled();
    expect(h.stopwatch.markInterim).not.toHaveBeenCalled();
    expect(h.stopwatch.startTimer).not.toHaveBeenCalled();
  });

  it("active langs read at dispatch time (not bind time)", () => {
    const dispatch = vi.fn();
    const stopwatch = {
      startTimer: vi.fn(), markInterim: vi.fn(), recordStt: vi.fn(),
      recordTranslate: vi.fn(), recordTts: vi.fn(), finalize: vi.fn(),
    };
    let langs = ["ja"];
    const handler = createMessageHandler(dispatch, () => langs, stopwatch);
    langs = ["zh", "en"];
    handler({ type: "final", utteranceId: 1, transcript: "x" });
    expect(stopwatch.startTimer).toHaveBeenCalledWith("1", "x", ["zh", "en"]);
  });
});
