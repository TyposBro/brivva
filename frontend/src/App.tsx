import { useState, useEffect, useRef } from "react";
import { useRealtimeTranslation } from "./hooks/useRealtimeTranslation";
import { AudioRecorder } from "./components/AudioRecorder";
import { AudioPlayer } from "./components/AudioPlayer";

const LANGUAGES = [
  { code: "ko", label: "Korean" },
  { code: "ja", label: "Japanese" },
  { code: "es", label: "Spanish" },
  { code: "fr", label: "French" },
  { code: "de", label: "German" },
  { code: "it", label: "Italian" },
  { code: "ru", label: "Russian" },
  { code: "zh", label: "Chinese" },
];

export default function App() {
  const [sourceLang, setSourceLang] = useState("ko");
  const { status, utterances, analyser, start, stop } = useRealtimeTranslation();
  const listRef = useRef<HTMLDivElement>(null);
  const isActive = status !== "idle";

  const handleToggle = () => {
    if (isActive) {
      stop();
    } else {
      start(sourceLang);
    }
  };

  // Auto-scroll utterance list as results arrive
  useEffect(() => {
    if (listRef.current) {
      listRef.current.scrollTop = listRef.current.scrollHeight;
    }
  }, [utterances]);

  // Map realtime status → RecorderState for the AudioRecorder component
  const recorderState =
    status === "idle"
      ? "idle"
      : status === "connecting"
        ? "processing"
        : "recording"; // listening + processing both show as active

  const latestTranslation = utterances[utterances.length - 1]?.translation ?? null;

  return (
    <div className="app">
      <header className="header">
        <h1 className="logo">brivva</h1>
        <p className="tagline">Real-time voice translation</p>
      </header>

      <main className="main">
        <div className="lang-row">
          <select
            className="lang-select"
            value={sourceLang}
            onChange={(e) => setSourceLang(e.target.value)}
            disabled={isActive}
          >
            {LANGUAGES.map((l) => (
              <option key={l.code} value={l.code}>
                {l.label}
              </option>
            ))}
          </select>
          <span className="arrow">→ English</span>
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
                  <span className="result-label">Original</span>
                  <p className="result-text">{u.transcription}</p>
                </div>
              )}
              {u.translation && (
                <div className="result-card translated">
                  <span className="result-label">Translation</span>
                  <p className="result-text">{u.translation}</p>
                </div>
              )}
            </div>
          ))}
        </div>

        {latestTranslation && <AudioPlayer key={latestTranslation} text={latestTranslation} lang="en" />}
      </main>
    </div>
  );
}
