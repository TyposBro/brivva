import { useEffect, useState, useCallback } from "react";
import { useNavigate, useSearchParams } from "react-router-dom";
import * as api from "../lib/api";

const LANGS = [
  { code: "ko", label: "Korean" },
  { code: "en", label: "English" },
  { code: "ja", label: "Japanese" },
  { code: "zh", label: "Chinese" },
];

function getUserId(): string {
  let id = localStorage.getItem("brivva_user_id");
  if (!id) {
    id = crypto.randomUUID();
    localStorage.setItem("brivva_user_id", id);
  }
  return id;
}

export default function DashboardPage() {
  const navigate = useNavigate();
  const [searchParams] = useSearchParams();
  const userId = getUserId();

  const [user, setUser] = useState<api.UserInfo | null>(null);
  const [voices, setVoices] = useState<api.Voice[]>([]);
  const [sessions, setSessions] = useState<api.Session[]>([]);
  const [loading, setLoading] = useState(true);

  // Session form
  const [title, setTitle] = useState("");
  const [sourceLang, setSourceLang] = useState("ko");
  const [targetLangs, setTargetLangs] = useState<string[]>(["en", "ja", "zh"]);
  const [selectedVoice, setSelectedVoice] = useState<string>("");
  const [creating, setCreating] = useState(false);
  const [error, setError] = useState("");

  const loadData = useCallback(async () => {
    try {
      const [u, v, s] = await Promise.all([
        api.getUser(userId),
        api.listVoices(userId),
        api.listSessions(userId),
      ]);
      setUser(u);
      setVoices(v.voices);
      setSessions(s.sessions);
    } catch (e) {
      console.error("Failed to load data:", e);
    } finally {
      setLoading(false);
    }
  }, [userId]);

  useEffect(() => {
    loadData();
  }, [loadData]);

  // Show success banner if redirected from YouTube OAuth
  const youtubeJustConnected = searchParams.get("youtube") === "connected";

  const toggleLang = (code: string) => {
    setTargetLangs((prev) =>
      prev.includes(code)
        ? prev.filter((l) => l !== code)
        : [...prev, code]
    );
  };

  const handleCreateSession = async () => {
    if (!title.trim()) {
      setError("Please enter a session title");
      return;
    }
    if (targetLangs.length === 0) {
      setError("Select at least one target language");
      return;
    }
    if (!user?.youtube_connected) {
      setError("Connect your YouTube account first");
      return;
    }

    setCreating(true);
    setError("");

    try {
      const result = await api.createSession({
        user_id: userId,
        title: title.trim(),
        source_lang: sourceLang,
        target_langs: targetLangs.filter((l) => l !== sourceLang),
        voice_id: selectedVoice || undefined,
      });

      if (result.error) {
        setError(result.error);
      }

      navigate(`/session/${result.session.id}`);
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : "Failed to create session");
      setCreating(false);
    }
  };

  const handleDeleteVoice = async (voiceId: string) => {
    await api.deleteVoice(voiceId);
    setVoices((prev) => prev.filter((v) => v.id !== voiceId));
    if (selectedVoice === voiceId) setSelectedVoice("");
  };

  if (loading) {
    return (
      <div className="app">
        <header className="header">
          <h1 className="logo">brivva</h1>
        </header>
        <main className="main">
          <p style={{ textAlign: "center", color: "var(--text-muted)" }}>Loading...</p>
        </main>
      </div>
    );
  }

  return (
    <div className="app">
      <header className="header">
        <h1 className="logo" onClick={() => navigate("/")} style={{ cursor: "pointer" }}>
          brivva
        </h1>
        <p className="tagline">Stream Dashboard</p>
      </header>

      <main className="main">
        {youtubeJustConnected && (
          <div className="dash-banner dash-banner--success">
            YouTube account connected successfully!
          </div>
        )}

        {/* YouTube Connection */}
        <section className="dash-section">
          <h2 className="dash-section-title">YouTube Account</h2>
          {user?.youtube_connected ? (
            <div className="dash-youtube-connected">
              <span className="dash-yt-icon">&#9654;</span>
              <span>{user.youtube_channel_name}</span>
              <span className="dash-badge dash-badge--success">Connected</span>
            </div>
          ) : (
            <a
              href={api.youtubeAuthUrl(userId)}
              className="record-btn dash-yt-connect-btn"
            >
              Connect YouTube Account
            </a>
          )}
        </section>

        {/* Saved Voices */}
        <section className="dash-section">
          <h2 className="dash-section-title">Saved Voices</h2>
          {voices.length === 0 ? (
            <p className="dash-empty">
              No saved voices yet. Record one by creating a room from the{" "}
              <span onClick={() => navigate("/host")} className="dash-link">
                host page
              </span>
              .
            </p>
          ) : (
            <div className="dash-voice-list">
              {voices.map((v) => (
                <div key={v.id} className="dash-voice-item">
                  <input
                    type="radio"
                    name="voice"
                    checked={selectedVoice === v.id}
                    onChange={() => setSelectedVoice(v.id)}
                  />
                  <span className="dash-voice-name">{v.name}</span>
                  <span className="dash-voice-date">
                    {new Date(v.created_at * 1000).toLocaleDateString()}
                  </span>
                  <button
                    className="dash-voice-delete"
                    onClick={() => handleDeleteVoice(v.id)}
                  >
                    &#10005;
                  </button>
                </div>
              ))}
            </div>
          )}
        </section>

        {/* Create Session */}
        <section className="dash-section">
          <h2 className="dash-section-title">Create Live Session</h2>

          <div className="dash-form">
            <label className="dash-label">
              Session Title
              <input
                className="dash-input"
                placeholder="My Live Stream"
                value={title}
                onChange={(e) => setTitle(e.target.value)}
              />
            </label>

            <label className="dash-label">
              Source Language (your language)
              <select
                className="dash-select"
                value={sourceLang}
                onChange={(e) => setSourceLang(e.target.value)}
              >
                {LANGS.map((l) => (
                  <option key={l.code} value={l.code}>
                    {l.label}
                  </option>
                ))}
              </select>
            </label>

            <fieldset className="dash-fieldset">
              <legend className="dash-legend">Target Languages</legend>
              <div className="dash-lang-grid">
                {LANGS.filter((l) => l.code !== sourceLang).map((l) => (
                  <label key={l.code} className="dash-lang-option">
                    <input
                      type="checkbox"
                      checked={targetLangs.includes(l.code)}
                      onChange={() => toggleLang(l.code)}
                    />
                    <span>{l.label}</span>
                  </label>
                ))}
              </div>
            </fieldset>

            {error && <p className="dash-error">{error}</p>}

            <button
              className="record-btn dash-go-live-btn"
              onClick={handleCreateSession}
              disabled={creating}
            >
              {creating ? "Creating streams..." : "Go Live"}
            </button>
          </div>
        </section>

        {/* Past Sessions */}
        {sessions.length > 0 && (
          <section className="dash-section">
            <h2 className="dash-section-title">Past Sessions</h2>
            <div className="dash-session-list">
              {sessions.map((s) => (
                <div
                  key={s.id}
                  className="dash-session-item"
                  onClick={() => navigate(`/session/${s.id}`)}
                >
                  <span className="dash-session-title">{s.title}</span>
                  <span className={`dash-badge dash-badge--${s.status}`}>
                    {s.status}
                  </span>
                  <span className="dash-session-date">
                    {new Date(s.created_at * 1000).toLocaleDateString()}
                  </span>
                </div>
              ))}
            </div>
          </section>
        )}
      </main>
    </div>
  );
}
