import { useEffect, useRef, useState } from "react";
import { useRealtimeTranslation } from "./hooks/useRealtimeTranslation";
import { AudioRecorder } from "./components/AudioRecorder";
import type { Timing } from "./hooks/useRealtimeTranslation";

const TARGET_MS = 300;

function LatencyPanel({ timing, isProcessing }: { timing: Timing | null; isProcessing: boolean }) {
  if (!timing?.ttsEndAt || !timing.translationAt || !timing.ttsStartAt) {
    return (
      <div className="latency-panel latency-panel--empty">
        <span className="lp-title">Pipeline Latency</span>
        <span className="lp-placeholder">
          {isProcessing ? <><span className="spinner lp-spinner" /> Processing…</> : "Speak to see latency breakdown"}
        </span>
      </div>
    );
  }

  const { sttStartAt, finalAt, translationAt, ttsStartAt, ttsEndAt } = timing;
  const start  = sttStartAt ?? finalAt;
  const total  = ttsEndAt - start;
  const sPhase = finalAt - start;
  const tPhase = translationAt - finalAt;
  const gPhase = ttsStartAt - translationAt;
  const aPhase = ttsEndAt - ttsStartAt;

  return (
    <div className="latency-panel">
      <div className="lp-header">
        <span className="lp-title">Pipeline Latency</span>
        {isProcessing && <span className="lp-active"><span className="spinner lp-spinner" /> Processing…</span>}
        <span className="lp-mult">{(total / TARGET_MS).toFixed(1)}× over &lt;{TARGET_MS}ms target</span>
      </div>
      <div className="latency-rows">
        <div className="latency-row">
          <span className="latency-lbl">Target</span>
          <div className="bar-track">
            <div className="bar-target" style={{ width: `${Math.min((TARGET_MS / total) * 100, 100)}%` }} />
          </div>
          <span className="bar-ms">{TARGET_MS}ms</span>
        </div>
        <div className="latency-row">
          <span className="latency-lbl">Actual</span>
          <div className="bar-track">
            <div className="bar-seg seg-stt"       style={{ width: `${(sPhase / total) * 100}%` }} />
            <div className="bar-seg seg-translate"  style={{ width: `${(tPhase / total) * 100}%` }} />
            <div className="bar-seg seg-tts"        style={{ width: `${(gPhase / total) * 100}%` }} />
            <div className="bar-seg seg-audio"      style={{ width: `${(aPhase / total) * 100}%` }} />
          </div>
          <span className="bar-ms">{total}ms</span>
        </div>
      </div>
      <div className="latency-legend">
        <span className="leg-item"><span className="leg-dot seg-stt"       />{sPhase}ms STT finalize</span>
        <span className="leg-item"><span className="leg-dot seg-translate"  />{tPhase}ms Translate</span>
        <span className="leg-item"><span className="leg-dot seg-tts"        />{gPhase}ms TTS Gen</span>
        <span className="leg-item"><span className="leg-dot seg-audio"      />{aPhase}ms Transfer</span>
      </div>
    </div>
  );
}

export default function App() {
  const { status, liveTranscript, utterances, analyser, start, stop, copyLog } =
    useRealtimeTranslation();
  const [copied, setCopied] = useState(false);
  const listRef = useRef<HTMLDivElement>(null);
  const isActive = status !== "idle";

  const handleToggle = () => (isActive ? stop() : start());

  const handleCopyLog = () => {
    copyLog();
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  useEffect(() => {
    if (listRef.current) {
      listRef.current.scrollTop = listRef.current.scrollHeight;
    }
  }, [utterances, liveTranscript]);

  const recorderState =
    status === "idle" ? "idle" : status === "connecting" ? "processing" : "recording";

  // Last fully completed utterance for the latency panel
  const lastCompleted = [...utterances].reverse().find(u => u.timing.ttsEndAt) ?? null;
  const isProcessing = isActive && utterances[utterances.length - 1]?.id !== lastCompleted?.id;

  return (
    <div className="app">
      <header className="header">
        <h1 className="logo">brivva</h1>
        <p className="tagline">Real-time voice translation</p>
      </header>

      <main className="main">
        <div className="lang-badge">
          <span>English</span>
          <span className="arrow">→</span>
          <span>French</span>
        </div>

        <AudioRecorder
          state={recorderState}
          analyser={analyser}
          onStart={handleToggle}
          onStop={handleToggle}
        />

        <LatencyPanel timing={lastCompleted?.timing ?? null} isProcessing={isProcessing} />

        <div className="utterances" ref={listRef}>
          {utterances.map((u) => (
            <div key={u.id} className="utterance">
              <div className="result-card">
                <span className="result-label">English</span>
                <p className="result-text">{u.transcript}</p>
              </div>
              {u.translation ? (
                <div className="result-card translated">
                  <span className="result-label">French</span>
                  <p className="result-text">{u.translation}</p>
                </div>
              ) : (
                <div className="result-card translating">
                  <span className="spinner" />
                </div>
              )}
            </div>
          ))}

          {/* Live interim transcript */}
          {liveTranscript && (
            <div className="utterance live">
              <div className="result-card live-card">
                <span className="result-label">
                  <span className="dot listening-dot" /> Live
                </span>
                <p className="result-text">{liveTranscript}</p>
              </div>
            </div>
          )}
        </div>
      </main>

      {utterances.length > 0 && (
        <button className="copy-log-btn" onClick={handleCopyLog}>
          {copied ? "✓ Copied!" : "Copy log"}
        </button>
      )}
    </div>
  );
}
