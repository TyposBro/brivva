import { describe, it, expect } from "vitest";
import { hostReducer, INITIAL_STATE, type HostState } from "./reducer";

const mkAnalyser = () => ({} as unknown as AnalyserNode);

describe("hostReducer", () => {
  describe("reset", () => {
    it("returns initial state with status=creating", () => {
      const prev: HostState = {
        ...INITIAL_STATE,
        status: "recording",
        liveTranscript: "hello",
        utterances: [{ id: 1, transcript: "x" }],
        error: "boom",
      };
      const next = hostReducer(prev, { type: "reset" });
      expect(next.status).toBe("creating");
      expect(next.liveTranscript).toBe("");
      expect(next.utterances).toEqual([]);
      expect(next.error).toBeNull();
    });
  });

  describe("connected", () => {
    it("idle → voice_setup", () => {
      const next = hostReducer(INITIAL_STATE, { type: "connected" });
      expect(next.status).toBe("voice_setup");
    });

    it("preserves other fields", () => {
      const prev = { ...INITIAL_STATE, liveTranscript: "keep me" };
      const next = hostReducer(prev, { type: "connected" });
      expect(next.liveTranscript).toBe("keep me");
    });
  });

  describe("interim", () => {
    it("updates liveTranscript", () => {
      const next = hostReducer(INITIAL_STATE, { type: "interim", transcript: "hel" });
      expect(next.liveTranscript).toBe("hel");
    });

    it("overwrites previous interim", () => {
      const s1 = hostReducer(INITIAL_STATE, { type: "interim", transcript: "hel" });
      const s2 = hostReducer(s1, { type: "interim", transcript: "hello" });
      expect(s2.liveTranscript).toBe("hello");
    });

    it("empty string allowed", () => {
      const prev = { ...INITIAL_STATE, liveTranscript: "x" };
      const next = hostReducer(prev, { type: "interim", transcript: "" });
      expect(next.liveTranscript).toBe("");
    });
  });

  describe("final", () => {
    it("clears liveTranscript + appends utterance", () => {
      const prev = { ...INITIAL_STATE, liveTranscript: "partial" };
      const next = hostReducer(prev, { type: "final", id: 1, transcript: "done" });
      expect(next.liveTranscript).toBe("");
      expect(next.utterances).toEqual([{ id: 1, transcript: "done" }]);
    });

    it("preserves order across multiple finals", () => {
      const s1 = hostReducer(INITIAL_STATE, { type: "final", id: 1, transcript: "a" });
      const s2 = hostReducer(s1, { type: "final", id: 2, transcript: "b" });
      expect(s2.utterances.map((u) => u.id)).toEqual([1, 2]);
    });
  });

  describe("translation", () => {
    it("adds per-lang translation", () => {
      const next = hostReducer(INITIAL_STATE, {
        type: "translation", id: 1, targetLang: "ja", text: "こんにちは",
      });
      expect(next.translations.ja).toEqual({ id: 1, text: "こんにちは" });
    });

    it("replaces previous translation for same lang", () => {
      const s1 = hostReducer(INITIAL_STATE, {
        type: "translation", id: 1, targetLang: "ja", text: "a",
      });
      const s2 = hostReducer(s1, {
        type: "translation", id: 2, targetLang: "ja", text: "b",
      });
      expect(s2.translations.ja).toEqual({ id: 2, text: "b" });
    });

    it("maintains separate lang slots", () => {
      const s1 = hostReducer(INITIAL_STATE, {
        type: "translation", id: 1, targetLang: "ja", text: "ja1",
      });
      const s2 = hostReducer(s1, {
        type: "translation", id: 1, targetLang: "zh", text: "zh1",
      });
      expect(s2.translations).toEqual({
        ja: { id: 1, text: "ja1" },
        zh: { id: 1, text: "zh1" },
      });
    });
  });

  describe("error", () => {
    it("sets error message; preserves status (sad path mid-flow)", () => {
      const prev = { ...INITIAL_STATE, status: "recording" as const };
      const next = hostReducer(prev, { type: "error", message: "WS crashed" });
      expect(next.error).toBe("WS crashed");
      expect(next.status).toBe("recording");
    });
  });

  describe("recording_started", () => {
    it("sets status=recording + analyser", () => {
      const a = mkAnalyser();
      const next = hostReducer(INITIAL_STATE, { type: "recording_started", analyser: a });
      expect(next.status).toBe("recording");
      expect(next.analyser).toBe(a);
    });
  });

  describe("recording_stopped", () => {
    it("recording → ready; clears analyser", () => {
      const prev = { ...INITIAL_STATE, status: "recording" as const, analyser: mkAnalyser() };
      const next = hostReducer(prev, { type: "recording_stopped" });
      expect(next.status).toBe("ready");
      expect(next.analyser).toBeNull();
    });

    it("non-recording status unchanged (sad: spurious stop)", () => {
      const prev = { ...INITIAL_STATE, status: "voice_setup" as const };
      const next = hostReducer(prev, { type: "recording_stopped" });
      expect(next.status).toBe("voice_setup");
      expect(next.analyser).toBeNull();
    });
  });

  describe("disconnected", () => {
    it("sets status=disconnected + clears analyser (sad path)", () => {
      const prev = { ...INITIAL_STATE, status: "recording" as const, analyser: mkAnalyser() };
      const next = hostReducer(prev, { type: "disconnected" });
      expect(next.status).toBe("disconnected");
      expect(next.analyser).toBeNull();
    });
  });

  describe("voice_cloning / voice_ready / skip_voice_setup", () => {
    it("voice_cloning sets status=cloning", () => {
      const prev = { ...INITIAL_STATE, status: "voice_setup" as const };
      const next = hostReducer(prev, { type: "voice_cloning" });
      expect(next.status).toBe("cloning");
    });

    it("voice_ready sets status=ready + voiceReady=true", () => {
      const prev = { ...INITIAL_STATE, status: "cloning" as const };
      const next = hostReducer(prev, { type: "voice_ready" });
      expect(next.status).toBe("ready");
      expect(next.voiceReady).toBe(true);
    });

    it("skip_voice_setup → ready but voiceReady stays false", () => {
      const prev = { ...INITIAL_STATE, status: "voice_setup" as const };
      const next = hostReducer(prev, { type: "skip_voice_setup" });
      expect(next.status).toBe("ready");
      expect(next.voiceReady).toBe(false);
    });
  });

  describe("media diagnostics + connection issue", () => {
    it("stores latest media diagnostics", () => {
      const diagnostics = {
        source: { width: 1280, height: 720, frameRate: 30 },
        outbound: { frameWidth: 1280, frameHeight: 720, framesPerSecond: 30 },
      };
      const next = hostReducer(INITIAL_STATE, { type: "media_diagnostics", diagnostics });
      expect(next.mediaDiagnostics).toBe(diagnostics);
    });

    it("stores connection issue message", () => {
      const next = hostReducer(INITIAL_STATE, { type: "connection_issue", message: "ICE failed" });
      expect(next.connectionIssue).toBe("ICE failed");
    });
  });

  describe("happy path full flow", () => {
    it("idle → creating → voice_setup → ready → recording → ready", () => {
      let s = INITIAL_STATE;
      s = hostReducer(s, { type: "reset" });
      expect(s.status).toBe("creating");
      s = hostReducer(s, { type: "connected" });
      expect(s.status).toBe("voice_setup");
      s = hostReducer(s, { type: "skip_voice_setup" });
      expect(s.status).toBe("ready");
      s = hostReducer(s, { type: "recording_started", analyser: mkAnalyser() });
      expect(s.status).toBe("recording");
      s = hostReducer(s, { type: "recording_stopped" });
      expect(s.status).toBe("ready");
    });
  });
});
