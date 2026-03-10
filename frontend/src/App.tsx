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
          <span>Japanese</span>
        </div>

        <AudioRecorder
          state={recorderState}
          analyser={analyser}
          onStart={handleToggle}
          onStop={handleToggle}
        />

        <div className="utterances" ref={listRef}>
          {utterances.map((u) => {
            const { finalAt, translationAt, translateMs, ttsEndAt, ttsMs } = u.timing;
            const translateTotal = translationAt ? translationAt - finalAt : null;
            const audioTotal = ttsEndAt ? ttsEndAt - finalAt : null;
            return (
              <div key={u.id} className="utterance">
                <div className="result-card">
                  <span className="result-label">English</span>
                  <p className="result-text">{u.transcript}</p>
                </div>
                {u.translation ? (
                  <div className="result-card translated">
                    <span className="result-label">Japanese</span>
                    <p className="result-text">{u.translation}</p>
                  </div>
                ) : (
                  <div className="result-card translating">
                    <span className="spinner" />
                  </div>
                )}
                {audioTotal && (
                  <div className="timing">
                    <span>translate: {translateTotal}ms {translateMs != null && <span className="timing-cf">(CF: {translateMs}ms)</span>}</span>
                    <span>audio: {audioTotal}ms {ttsMs != null && <span className="timing-cf">(CF: {ttsMs}ms)</span>}</span>
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
