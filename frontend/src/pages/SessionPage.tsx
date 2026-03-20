import { useEffect, useState, useCallback } from "react";
import { useParams, useNavigate } from "react-router-dom";
import * as api from "../lib/api";

export default function SessionPage() {
  const { id } = useParams<{ id: string }>();
  const navigate = useNavigate();

  const [session, setSession] = useState<api.Session | null>(null);
  const [streams, setStreams] = useState<api.StreamInfo[]>([]);
  const [loading, setLoading] = useState(true);
  const [ending, setEnding] = useState(false);

  const loadSession = useCallback(async () => {
    if (!id) return;
    try {
      const data = await api.getSession(id);
      setSession(data.session);
      setStreams(data.streams);
    } catch (e) {
      console.error("Failed to load session:", e);
    } finally {
      setLoading(false);
    }
  }, [id]);

  useEffect(() => {
    loadSession();
  }, [loadSession]);

  const handleEnd = async () => {
    if (!id || ending) return;
    setEnding(true);
    try {
      await api.deleteSession(id);
      await loadSession();
    } catch (e) {
      console.error("Failed to end session:", e);
    } finally {
      setEnding(false);
    }
  };

  if (loading) {
    return (
      <div className="app">
        <header className="header">
          <h1 className="logo">brivva</h1>
        </header>
        <main className="main">
          <p style={{ textAlign: "center", color: "var(--text-muted)" }}>Loading session...</p>
        </main>
      </div>
    );
  }

  if (!session) {
    return (
      <div className="app">
        <header className="header">
          <h1 className="logo">brivva</h1>
        </header>
        <main className="main">
          <p style={{ textAlign: "center", color: "var(--error)" }}>Session not found</p>
          <button className="record-btn" onClick={() => navigate("/dashboard")} style={{ marginTop: "1rem" }}>
            Back to Dashboard
          </button>
        </main>
      </div>
    );
  }

  const targetLangs: string[] = (() => {
    try {
      return JSON.parse(session.target_langs);
    } catch {
      return [];
    }
  })();

  const isLive = session.status === "live";

  return (
    <div className="app">
      <header className="header">
        <h1 className="logo" onClick={() => navigate("/dashboard")} style={{ cursor: "pointer" }}>
          brivva
        </h1>
        <p className="tagline">Live Session</p>
      </header>

      <main className="main">
        {/* Session Info */}
        <section className="dash-section">
          <div className="session-header">
            <h2 className="session-title">{session.title}</h2>
            <span className={`dash-badge dash-badge--${session.status}`}>
              {session.status}
            </span>
          </div>
          <div className="session-meta">
            <span>Source: {session.source_lang.toUpperCase()}</span>
            <span>Targets: {targetLangs.map((l) => l.toUpperCase()).join(", ")}</span>
          </div>
        </section>

        {/* Stream Cards */}
        <section className="dash-section">
          <h2 className="dash-section-title">Live Streams</h2>
          <div className="stream-grid">
            {streams.map((s) => (
              <div key={s.id} className="stream-card">
                <div className="stream-card-header">
                  <div style={{ display: "flex", alignItems: "center", gap: "0.5rem" }}>
                    <span className="stream-lang">{s.lang?.toUpperCase()}</span>
                    {s.platform && (
                      <span className="stream-platform">{s.platform}</span>
                    )}
                  </div>
                  <span
                    className={`dash-badge ${
                      s.error
                        ? "dash-badge--error"
                        : s.status === "ready"
                        ? "dash-badge--success"
                        : "dash-badge--setup"
                    }`}
                  >
                    {s.error ? "Error" : s.status ?? "pending"}
                  </span>
                </div>

                {s.error && <p className="stream-error">{s.error}</p>}

                {s.broadcast_id && (
                  <div className="stream-detail">
                    <span className="stream-detail-label">Broadcast</span>
                    <code className="stream-detail-value">{s.broadcast_id}</code>
                  </div>
                )}

                {s.rtmp_url && (
                  <div className="stream-detail">
                    <span className="stream-detail-label">RTMP</span>
                    <code className="stream-detail-value stream-rtmp">{s.rtmp_url}</code>
                  </div>
                )}
              </div>
            ))}
          </div>
        </section>

        {/* Actions */}
        <section className="dash-section session-actions">
          {isLive && !session.room_id && (
            <button
              className="record-btn dash-go-live-btn"
              onClick={() => navigate(`/host?sessionId=${session.id}`)}
            >
              Start Broadcasting
            </button>
          )}
          {isLive && session.room_id && (
            <div className="session-room-info">
              <span className="session-room-label">Room Code:</span>
              <code className="session-room-code">{session.room_id}</code>
            </div>
          )}
          {isLive && (
            <button
              className="record-btn dash-end-btn"
              onClick={handleEnd}
              disabled={ending}
            >
              {ending ? "Ending..." : "End Session"}
            </button>
          )}
          <button
            className="record-btn dash-back-btn"
            onClick={() => navigate("/dashboard")}
          >
            Back to Dashboard
          </button>
        </section>
      </main>
    </div>
  );
}
