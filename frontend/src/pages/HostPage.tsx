import { useEffect, useRef, useState, useCallback } from "react";
import { useNavigate, useSearchParams } from "react-router-dom";
import { useHostRoom } from "../hooks/useHostRoom";
import { AudioRecorder } from "../components/AudioRecorder";
import { LatencyDashboard } from "../components/LatencyDashboard";
import * as api from "../lib/api";

export default function HostPage() {
  const navigate = useNavigate();
  const [searchParams] = useSearchParams();
  const sessionId = searchParams.get("sessionId") ?? undefined;
  const {
    status, liveTranscript, utterances,
    analyser, error, timings, videoRef,
    createRoom, startRecording, stopRecording, closeRoom,
    startVoiceRecording, skipVoiceSetup,
  } = useHostRoom();

  const listRef = useRef<HTMLDivElement>(null);
  const createdRef = useRef(false);

  // Session + streams info
  const [session, setSession] = useState<api.Session | null>(null);
  const [streams, setStreams] = useState<api.StreamInfo[]>([]);

  // Load session info if sessionId provided
  const loadSession = useCallback(async () => {
    if (!sessionId) return;
    try {
      const data = await api.getSession(sessionId);
      setSession(data.session);
      setStreams(data.streams);
    } catch (e) {
      console.error("Failed to load session:", e);
    }
  }, [sessionId]);

  useEffect(() => {
    loadSession();
  }, [loadSession]);

  useEffect(() => {
    if (createdRef.current) return;
    createdRef.current = true;
    createRoom({ sessionId });
  }, [createRoom, sessionId]);

  useEffect(() => {
    if (listRef.current) {
      listRef.current.scrollTop = listRef.current.scrollHeight;
    }
  }, [utterances, liveTranscript]);

  const handleBack = () => {
    closeRoom();
    navigate(sessionId ? `/session/${sessionId}` : "/dashboard");
  };

  // Voice recording timer
  const [voiceTimer, setVoiceTimer] = useState(0);
  const [isVoiceRecording, setIsVoiceRecording] = useState(false);
  const voiceTimerRef = useRef<ReturnType<typeof setInterval> | null>(null);

  const handleStartVoice = () => {
    setVoiceTimer(30);
    setIsVoiceRecording(true);
    startVoiceRecording();
    voiceTimerRef.current = setInterval(() => {
      setVoiceTimer((t) => {
        if (t <= 1) {
          clearInterval(voiceTimerRef.current!);
          setIsVoiceRecording(false);
          return 0;
        }
        return t - 1;
      });
    }, 1000);
  };

  const isReady = status === "ready" || status === "recording";
  const isRecording = status === "recording";

  // Find platform label from PLATFORMS array
  const getPlatformLabel = (platformId: string) => {
    const p = api.PLATFORMS.find((pl) => pl.id === platformId);
    return p?.label ?? platformId;
  };

  return (
    <div className="app">
      <header className="header">
        <button className="back-btn" onClick={handleBack}>← Back</button>
        <h1 className="logo">brivva</h1>
        <p className="tagline">
          {session ? session.title : "Host · English"}
        </p>
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

        {/* Stream Status Cards */}
        {isReady && streams.length > 0 && (
          <div className="host-streams-panel">
            <div className="host-streams-header">
              <span className="host-streams-title">Live Streams</span>
              <span className="host-streams-count">{streams.length} platform{streams.length !== 1 ? "s" : ""}</span>
            </div>
            <div className="host-streams-grid">
              {streams.map((s) => (
                <div key={s.id} className="host-stream-card">
                  <div className="host-stream-top">
                    <span className="host-stream-lang">{s.lang?.toUpperCase()}</span>
                    <span className="host-stream-platform">{getPlatformLabel(s.platform ?? "custom")}</span>
                    <span className={`host-stream-status ${s.error ? "host-stream-error" : "host-stream-live"}`}>
                      {s.error ? "ERR" : isRecording ? "LIVE" : "READY"}
                    </span>
                  </div>
                  {s.error && <span className="host-stream-err-msg">{s.error}</span>}
                </div>
              ))}
            </div>
          </div>
        )}

        {/* Voice Setup Phase */}
        {status === "voice_setup" && (
          <div className="voice-setup-panel">
            <h3>Voice Setup</h3>
            <p className="voice-setup-desc">
              Record a 30-second voice sample to clone your voice. Read the text below naturally at your normal pace.
            </p>
            {!isVoiceRecording && voiceTimer === 0 && (
              <button className="voice-record-btn" onClick={handleStartVoice}>
                Record Voice Sample
              </button>
            )}
            {isVoiceRecording && (
              <>
                <div className="voice-recording-indicator">
                  <span className="dot listening-dot" />
                  <span>Recording... {voiceTimer}s</span>
                </div>
                <div className="voice-script">
                  Welcome to today's live stream! I'm really excited to show you
                  some amazing products that I've been using lately. These items
                  have completely changed my daily routine, and I think you're going
                  to love them too. The quality is outstanding, and the price is
                  incredibly reasonable for what you get. I've tried many similar
                  products before, but nothing comes close to this. If you have any
                  questions, feel free to drop them in the chat and I'll answer them
                  right away. Let's get started!
                </div>
              </>
            )}
            <button className="voice-skip-btn" onClick={skipVoiceSetup}>
              Skip (use default voice)
            </button>
          </div>
        )}

        {status === "cloning" && (
          <div className="voice-setup-panel">
            <div className="status-bar">
              <span className="spinner" /> Cloning your voice...
            </div>
          </div>
        )}

        {/* Host webcam preview (mirrored) — always rendered so ref is available */}
        <div className="webcam-preview" style={{ display: isReady ? "flex" : "none" }}>
          <video
            ref={videoRef}
            autoPlay
            muted
            playsInline
            style={{
              width: "256px",
              height: "256px",
              objectFit: "cover",
              transform: "scaleX(-1)",
              borderRadius: "12px",
              border: "2px solid #333",
            }}
          />
          <span className="webcam-label">Your camera (mirrored)</span>
        </div>

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
                <span className="result-label">
                  {session?.source_lang?.toUpperCase() ?? "EN"}
                </span>
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
      </main>
    </div>
  );
}
