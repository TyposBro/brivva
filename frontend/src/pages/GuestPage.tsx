import { useState, useRef, useEffect } from "react";
import { useParams, useSearchParams } from "react-router-dom";
import { useGuestRoom } from "../hooks/useGuestRoom";
import { type Lang, LANGS, LANG_LABELS } from "../types";

export default function GuestPage() {
  const { id = "" } = useParams<{ id: string }>();
  const [searchParams] = useSearchParams();
  const urlLang = searchParams.get("lang");
  const validUrlLang = urlLang && LANGS.includes(urlLang as Lang) ? (urlLang as Lang) : null;

  const [selectedLang, setSelectedLang] = useState<Lang | null>(validUrlLang);

  const { status, liveTranscript, utterances, error, attachCanvas } = useGuestRoom(id, selectedLang);

  const listRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (listRef.current) {
      listRef.current.scrollTop = listRef.current.scrollHeight;
    }
  }, [utterances, liveTranscript]);

  // Language picker screen
  if (!selectedLang) {
    return (
      <div className="app">
        <header className="header">
          <h1 className="logo">brivva</h1>
          <p className="tagline">Room {id}</p>
        </header>
        <main className="main">
          <p className="lang-pick-label">Choose your language</p>
          <div className="lang-picker">
            {LANGS.map((lang) => (
              <button
                key={lang}
                className="lang-pick-btn"
                onClick={() => {
                  // Unlock browser audio on user gesture so later audio.play() calls succeed
                  const ctx = new AudioContext();
                  ctx.resume().then(() => ctx.close());
                  setSelectedLang(lang);
                }}
              >
                {LANG_LABELS[lang]}
              </button>
            ))}
          </div>
        </main>
      </div>
    );
  }

  const tagline =
    status === "connecting" ? "Connecting…"
    : status === "listening" ? `Listening · ${LANG_LABELS[selectedLang]}`
    : status === "closed" ? "Host disconnected"
    : status === "error" ? "Connection error"
    : "";

  return (
    <div className="app">
      <header className="header">
        <h1 className="logo">brivva</h1>
        <p className="tagline">{tagline}</p>
      </header>

      <main className="main">
        {error && <div className="error">{error}</div>}

        {status === "connecting" && (
          <div className="status-bar">
            <span className="spinner" /> Joining room {id}…
          </div>
        )}

        {status === "closed" && (
          <div className="status-bar">The host has ended this session.</div>
        )}

        {/* Live host video */}
        {status === "listening" && (
          <div className="host-video">
            <canvas
              ref={attachCanvas}
              width={256}
              height={256}
              style={{
                width: "256px",
                height: "256px",
                borderRadius: "12px",
                border: "2px solid #333",
                background: "#1a1a1a",
              }}
            />
            <span className="host-video-label">Host (live)</span>
          </div>
        )}

        <div className="utterances" ref={listRef}>
          {utterances.map((u) => (
            <div key={u.id} className="utterance">
              <div className="result-card">
                <span className="result-label">Korean</span>
                <p className="result-text">{u.original}</p>
              </div>
              {u.translation ? (
                <div className="result-card translated">
                  <span className="result-label">{LANG_LABELS[selectedLang]}</span>
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
    </div>
  );
}
