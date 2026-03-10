import { useEffect, useRef } from "react";
import { useRealtimeTranslation } from "./hooks/useRealtimeTranslation";
import { AudioRecorder } from "./components/AudioRecorder";
import { AudioPlayer } from "./components/AudioPlayer";

export default function App() {
  const { status, utterances, analyser, start, stop } = useRealtimeTranslation();
  const listRef = useRef<HTMLDivElement>(null);
  const isActive = status !== "idle";

  const handleToggle = () => (isActive ? stop() : start());

  useEffect(() => {
    if (listRef.current) {
      listRef.current.scrollTop = listRef.current.scrollHeight;
    }
  }, [utterances]);

  const recorderState =
    status === "idle" ? "idle" : status === "connecting" ? "processing" : "recording";

  const latestTranslation = utterances[utterances.length - 1]?.translation ?? null;

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
          <span>Spanish</span>
        </div>

        <AudioRecorder
          state={recorderState}
          analyser={analyser}
          onStart={handleToggle}
          onStop={handleToggle}
        />

        {status === "listening" && (
          <div className="status-bar">
            <span className="dot listening-dot" /> Listening...
          </div>
        )}
        {status === "processing" && (
          <div className="status-bar">
            <span className="spinner" /> Translating...
          </div>
        )}

        <div className="utterances" ref={listRef}>
          {utterances.map((u) => (
            <div key={u.id} className="utterance">
              {u.transcription && (
                <div className="result-card">
                  <span className="result-label">English</span>
                  <p className="result-text">{u.transcription}</p>
                </div>
              )}
              {u.translation && (
                <div className="result-card translated">
                  <span className="result-label">Spanish</span>
                  <p className="result-text">{u.translation}</p>
                </div>
              )}
            </div>
          ))}
        </div>

        {latestTranslation && (
          <AudioPlayer key={latestTranslation} text={latestTranslation} lang="es" />
        )}
      </main>
    </div>
  );
}
