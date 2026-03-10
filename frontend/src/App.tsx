import { useEffect, useRef, useState } from "react";
import { useRealtimeTranslation } from "./hooks/useRealtimeTranslation";
import { AudioRecorder } from "./components/AudioRecorder";

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

        <div className="utterances" ref={listRef}>
          {utterances.map((u) => {
            const { sttStartAt, finalAt, translationAt, ttsStartAt, ttsEndAt } = u.timing;
            const hasLatency = ttsEndAt && translationAt && ttsStartAt;
            const start    = sttStartAt ?? finalAt;
            const total    = hasLatency ? ttsEndAt - start : 0;
            const sPhase   = hasLatency ? finalAt - start : 0;
            const tPhase   = hasLatency ? translationAt - finalAt : 0;
            const gPhase   = hasLatency ? ttsStartAt - translationAt : 0;
            const aPhase   = hasLatency ? ttsEndAt - ttsStartAt : 0;
            const TARGET   = 300;
            return (
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
                {hasLatency && (
                  <div className="latency-dashboard">
                    <div className="latency-header">
                      <span className="latency-total">{total}ms total</span>
                      <span className="latency-mult">{(total / TARGET).toFixed(1)}× over &lt;300ms target</span>
                    </div>
                    <div className="latency-rows">
                      <div className="latency-row">
                        <span className="latency-lbl">Target</span>
                        <div className="bar-track">
                          <div className="bar-target" style={{ width: `${Math.min((TARGET / total) * 100, 100)}%` }} />
                        </div>
                        <span className="bar-ms">300ms</span>
                      </div>
                      <div className="latency-row">
                        <span className="latency-lbl">Actual</span>
                        <div className="bar-track">
                          <div className="bar-seg seg-stt" style={{ width: `${(sPhase / total) * 100}%` }} />
                          <div className="bar-seg seg-translate" style={{ width: `${(tPhase / total) * 100}%` }} />
                          <div className="bar-seg seg-tts" style={{ width: `${(gPhase / total) * 100}%` }} />
                          <div className="bar-seg seg-audio" style={{ width: `${(aPhase / total) * 100}%` }} />
                        </div>
                        <span className="bar-ms">{total}ms</span>
                      </div>
                    </div>
                    <div className="latency-legend">
                      <span className="leg-item"><span className="leg-dot seg-stt" />{sPhase}ms STT</span>
                      <span className="leg-item"><span className="leg-dot seg-translate" />{tPhase}ms Translate</span>
                      <span className="leg-item"><span className="leg-dot seg-tts" />{gPhase}ms TTS Gen</span>
                      <span className="leg-item"><span className="leg-dot seg-audio" />{aPhase}ms Transfer</span>
                    </div>
                  </div>
                )}
              </div>
            );
          })}

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
