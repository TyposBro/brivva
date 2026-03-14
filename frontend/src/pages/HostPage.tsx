import { useEffect, useRef } from "react";
import { useNavigate } from "react-router-dom";
import { useHostRoom } from "../hooks/useHostRoom";
import { AudioRecorder } from "../components/AudioRecorder";
import { LatencyDashboard } from "../components/LatencyDashboard";
import { PipelineAnalysis } from "../components/PipelineAnalysis";

export default function HostPage() {
  const navigate = useNavigate();
  const {
    status, roomId, guestCounts, liveTranscript, utterances,
    analyser, error, timings, createRoom, startRecording, stopRecording, closeRoom,
  } = useHostRoom();

  const listRef = useRef<HTMLDivElement>(null);
  const createdRef = useRef(false);

  useEffect(() => {
    if (createdRef.current) return;
    createdRef.current = true;
    createRoom();
  }, [createRoom]);

  useEffect(() => {
    if (listRef.current) {
      listRef.current.scrollTop = listRef.current.scrollHeight;
    }
  }, [utterances, liveTranscript]);

  const handleBack = () => {
    closeRoom();
    navigate("/");
  };

  const isReady = status === "ready" || status === "recording";
  const isRecording = status === "recording";
  const shareUrl = roomId ? `${window.location.origin}/room/${roomId}` : "";

  const copyLink = () => {
    navigator.clipboard.writeText(shareUrl).catch(() => {});
  };

  return (
    <div className="app">
      <header className="header">
        <button className="back-btn" onClick={handleBack}>← Back</button>
        <h1 className="logo">brivva</h1>
        <p className="tagline">Host · English</p>
      </header>

      <main className="main">
        {error && <div className="error">{error}</div>}

        {status === "creating" && (
          <div className="status-bar">
            <span className="spinner" /> Creating room…
          </div>
        )}

        {status === "disconnected" && (
          <div className="status-bar">Disconnected.</div>
        )}

        {roomId && (
          <div className="room-code-panel">
            <span className="room-code-label">Room Code</span>
            <span className="room-code">{roomId}</span>
            <button className="copy-link-btn" onClick={copyLink}>
              Copy link
            </button>
          </div>
        )}

        {isReady && (
          <div className="guest-count-bar">
            <span className="gc-item">
              <span className="gc-flag">EN</span>
              <span className="gc-num">{guestCounts.en}</span>
            </span>
            <span className="gc-sep" />
            <span className="gc-item">
              <span className="gc-flag">JA</span>
              <span className="gc-num">{guestCounts.ja}</span>
            </span>
            <span className="gc-sep" />
            <span className="gc-item">
              <span className="gc-flag">ZH</span>
              <span className="gc-num">{guestCounts.zh}</span>
            </span>
          </div>
        )}

        {isReady && (
          <div className="lang-badge">
            <span>English</span>
            <span className="arrow">→</span>
            <span>日本語 / 中文</span>
          </div>
        )}

        {isReady && (
          <AudioRecorder
            isRecording={isRecording}
            analyser={analyser}
            onStart={startRecording}
            onStop={stopRecording}
          />
        )}

        <div className="utterances" ref={listRef}>
          {utterances.map((u) => (
            <div key={u.id} className="utterance">
              <div className="result-card">
                <span className="result-label">English</span>
                <p className="result-text">{u.transcript}</p>
              </div>
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

        <LatencyDashboard timings={timings} />

        <PipelineAnalysis />
      </main>
    </div>
  );
}
