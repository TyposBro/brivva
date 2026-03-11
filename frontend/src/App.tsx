import { useEffect, useRef, useState } from "react";
import { useRealtimeTranslation } from "./hooks/useRealtimeTranslation";
import { AudioRecorder } from "./components/AudioRecorder";
import type { Utterance } from "./hooks/useRealtimeTranslation";

const TARGET_MS = 300;
const SCALE_MS  = 5000; // fixed bar scale — all utterances comparable

function LatencyPanel({ utterances, isProcessing }: { utterances: Utterance[]; isProcessing: boolean }) {
  const completed = utterances.filter(
    u => u.timing.ttsEndAt && u.timing.translationAt && u.timing.ttsStartAt
  );

  return (
    <div className="latency-panel">
      <div className="lp-header">
        <span className="lp-title">Pipeline Latency</span>
        {isProcessing && <span className="lp-active"><span className="spinner lp-spinner" /> Processing…</span>}
        <span className="lp-scale-label">{SCALE_MS / 1000}s max scale · target {TARGET_MS}ms</span>
      </div>

      {completed.length === 0 ? (
        <span className="lp-placeholder">
          {isProcessing ? "Waiting for first utterance…" : "Speak to see latency breakdown"}
        </span>
      ) : (
        <>
          <div className="lp-bars">
            {/* Target reference line header */}
            <div className="lp-target-line" style={{ left: `${(TARGET_MS / SCALE_MS) * 100}%` }}>
              <span className="lp-target-label">▲ 300ms</span>
            </div>

            {completed.map((u, i) => {
              const { sttStartAt, finalAt, translationAt, ttsStartAt, ttsEndAt } = u.timing;
              const start  = sttStartAt ?? finalAt;
              const total  = ttsEndAt! - start;
              const sPhase = finalAt - start;
              const tPhase = translationAt! - finalAt;
              const gPhase = ttsStartAt! - translationAt!;
              const aPhase = ttsEndAt! - ttsStartAt!;

              return (
                <div key={u.id} className="lp-row">
                  <span className="lp-row-label">#{i + 1}</span>
                  <div className="lp-bar-wrap">
                    <div className="lp-bar">
                      <div className="bar-seg seg-stt"       style={{ width: `${(sPhase / SCALE_MS) * 100}%` }} />
                      <div className="bar-seg seg-translate"  style={{ width: `${(tPhase / SCALE_MS) * 100}%` }} />
                      <div className="bar-seg seg-tts"        style={{ width: `${(gPhase / SCALE_MS) * 100}%` }} />
                      <div className="bar-seg seg-audio"      style={{ width: `${(aPhase / SCALE_MS) * 100}%` }} />
                    </div>
                    <div className="lp-target-marker" style={{ left: `${(TARGET_MS / SCALE_MS) * 100}%` }} />
                  </div>
                  <span className="lp-row-ms">{total}ms</span>
                </div>
              );
            })}
          </div>

          <div className="latency-legend">
            <span className="leg-item"><span className="leg-dot seg-stt"       />STT finalize</span>
            <span className="leg-item"><span className="leg-dot seg-translate"  />Translate</span>
            <span className="leg-item"><span className="leg-dot seg-tts"        />TTS Gen</span>
            <span className="leg-item"><span className="leg-dot seg-audio"      />Transfer</span>
          </div>
        </>
      )}
    </div>
  );
}

export default function App() {
  const { status, liveTranscript, utterances, analyser, start, stop, clear, copyLog } =
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

  // Fix: "connecting" used to map to "processing" which disabled the Stop button
  const recorderState = status === "idle" ? "idle" : "recording";

  const isProcessing = isActive && !utterances[utterances.length - 1]?.timing.ttsEndAt;

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

        {isActive && utterances.length > 0 && (
          <button className="clear-btn" onClick={clear}>Clear</button>
        )}

        <LatencyPanel utterances={utterances} isProcessing={isProcessing} />

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
