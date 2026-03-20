import { useEffect, useState, useCallback } from "react";
import { useNavigate, useSearchParams } from "react-router-dom";
import * as api from "../lib/api";

const LANGS = [
  { code: "ko", label: "Korean" },
  { code: "en", label: "English" },
  { code: "ja", label: "Japanese" },
  { code: "zh", label: "Chinese" },
];

type PlatformEntry = {
  platform: string;
  rtmp_url: string;
  stream_key: string;
};

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
  const [savedCreds, setSavedCreds] = useState<Record<string, api.PlatformCredential>>({});

  // Session form
  const [title, setTitle] = useState("");
  const [sourceLang, setSourceLang] = useState("ko");
  const [targetLang, setTargetLang] = useState("en");
  const [selectedVoice, setSelectedVoice] = useState<string>("");
  const [creating, setCreating] = useState(false);
  const [error, setError] = useState("");

  // Platform — single selection
  const [selectedPlatform, setSelectedPlatform] = useState<string>("");
  const [platformConfig, setPlatformConfig] = useState<PlatformEntry>({ platform: "", rtmp_url: "", stream_key: "" });
  const [magicPaste, setMagicPaste] = useState("");

  const loadData = useCallback(async () => {
    try {
      const [u, v, s, c] = await Promise.all([
        api.getUser(userId),
        api.listVoices(userId),
        api.listSessions(userId),
        api.listCredentials(userId),
      ]);
      setUser(u);
      setVoices(v.voices);
      setSessions(s.sessions);
      const credMap: Record<string, api.PlatformCredential> = {};
      for (const cred of c.credentials) {
        credMap[cred.platform] = cred;
      }
      setSavedCreds(credMap);
    } catch (e) {
      console.error("Failed to load data:", e);
    } finally {
      setLoading(false);
    }
  }, [userId]);

  useEffect(() => {
    loadData();
  }, [loadData]);

  const youtubeJustConnected = searchParams.get("youtube") === "connected";

  const selectPlatform = (id: string) => {
    setSelectedPlatform(id);
    // Pre-fill from saved credentials
    const cred = savedCreds[id];
    const p = api.PLATFORMS.find((x) => x.id === id);
    setPlatformConfig({
      platform: id,
      rtmp_url: cred?.rtmp_url ?? p?.defaultRtmp ?? "",
      stream_key: cred?.stream_key ?? "",
    });
  };

  const handleMagicPaste = (value: string) => {
    setMagicPaste(value);
    const detected = api.detectPlatform(value);
    if (detected) {
      setSelectedPlatform(detected.platform);
      setPlatformConfig({
        platform: detected.platform,
        rtmp_url: detected.rtmpUrl,
        stream_key: detected.streamKey,
      });
      setMagicPaste("");
    }
  };

  const handleCreateSession = async () => {
    if (!title.trim()) { setError("Please enter a session title"); return; }
    if (!selectedPlatform) { setError("Select a platform"); return; }

    const p = api.PLATFORMS.find((x) => x.id === selectedPlatform);

    if (selectedPlatform === "youtube" && !user?.youtube_connected) {
      setError("Connect your YouTube account first");
      return;
    }

    if (p && !p.auto) {
      if (!p.keyOnly && !platformConfig.rtmp_url && !p.defaultRtmp) {
        setError(`Enter server URL for ${p.label}`);
        return;
      }
      if (!platformConfig.stream_key) {
        setError(`Enter stream key for ${p.label}`);
        return;
      }
    }

    setCreating(true);
    setError("");

    try {
      const platforms: api.PlatformConfig[] = [];
      if (p?.auto) {
        platforms.push({ platform: selectedPlatform });
      } else {
        const rtmpUrl = p?.keyOnly ? p.defaultRtmp : (platformConfig.rtmp_url || p?.defaultRtmp || "");
        platforms.push({
          platform: selectedPlatform,
          rtmp_url: rtmpUrl,
          stream_key: platformConfig.stream_key,
        });
      }

      const result = await api.createSession({
        user_id: userId,
        title: title.trim(),
        source_lang: sourceLang,
        target_langs: [targetLang],
        voice_id: selectedVoice || undefined,
        platforms,
      });

      if (result.errors?.length) {
        setError(result.errors.join("; "));
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
        <header className="header"><h1 className="logo">brivva</h1></header>
        <main className="main">
          <p style={{ textAlign: "center", color: "var(--text-muted)" }}>Loading...</p>
        </main>
      </div>
    );
  }

  const activePlatform = api.PLATFORMS.find((x) => x.id === selectedPlatform);

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
            <a href={api.youtubeAuthUrl(userId)} className="record-btn dash-yt-connect-btn">
              Connect YouTube Account
            </a>
          )}
        </section>

        {/* Saved Voices */}
        <section className="dash-section">
          <h2 className="dash-section-title">Saved Voices</h2>
          {voices.length === 0 ? (
            <p className="dash-empty">No saved voices yet. You can record one when starting a broadcast.</p>
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
                  <button className="dash-voice-delete" onClick={() => handleDeleteVoice(v.id)}>
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

            <div style={{ display: "flex", gap: "0.75rem" }}>
              <label className="dash-label" style={{ flex: 1 }}>
                You speak
                <select
                  className="dash-select"
                  value={sourceLang}
                  onChange={(e) => {
                    setSourceLang(e.target.value);
                    if (e.target.value === targetLang) {
                      const other = LANGS.find((l) => l.code !== e.target.value);
                      if (other) setTargetLang(other.code);
                    }
                  }}
                >
                  {LANGS.map((l) => (
                    <option key={l.code} value={l.code}>{l.label}</option>
                  ))}
                </select>
              </label>

              <label className="dash-label" style={{ flex: 1 }}>
                Translate to
                <select
                  className="dash-select"
                  value={targetLang}
                  onChange={(e) => setTargetLang(e.target.value)}
                >
                  {LANGS.filter((l) => l.code !== sourceLang).map((l) => (
                    <option key={l.code} value={l.code}>{l.label}</option>
                  ))}
                </select>
              </label>
            </div>

            {/* Platform Selection — single */}
            <fieldset className="dash-fieldset">
              <legend className="dash-legend">Stream To</legend>
              <div className="dash-magic-paste">
                <input
                  className="dash-input"
                  placeholder="Paste any RTMP URL to auto-detect platform..."
                  value={magicPaste}
                  onChange={(e) => handleMagicPaste(e.target.value)}
                />
              </div>
              <div className="dash-platform-list">
                {["Global", "Korea", "Japan", "China", "Other"].map((region) => {
                  const regionPlatforms = api.PLATFORMS.filter((p) => p.region === region);
                  if (regionPlatforms.length === 0) return null;
                  return (
                    <div key={region} className="dash-platform-region">
                      <div className="dash-platform-region-label">{region}</div>
                      {regionPlatforms.map((p) => {
                        const selected = selectedPlatform === p.id;
                        return (
                          <div key={p.id} className="dash-platform-item">
                            <label className="dash-platform-toggle">
                              <input
                                type="radio"
                                name="platform"
                                checked={selected}
                                onChange={() => selectPlatform(p.id)}
                              />
                              <span className="dash-platform-label">{p.label}</span>
                              {p.auto && selected && (
                                <span className="dash-badge dash-badge--success" style={{ marginLeft: "0.5rem" }}>
                                  Auto
                                </span>
                              )}
                            </label>
                          </div>
                        );
                      })}
                    </div>
                  );
                })}
              </div>

              {/* Config for selected platform */}
              {activePlatform && !activePlatform.auto && (
                <div className="dash-platform-expand" style={{ marginTop: "0.75rem" }}>
                  {savedCreds[selectedPlatform] && (
                    <div className="dash-platform-saved">Saved from last session</div>
                  )}
                  {activePlatform.settingsUrl && (
                    <a
                      href={activePlatform.settingsUrl}
                      target="_blank"
                      rel="noopener noreferrer"
                      className="dash-platform-deeplink"
                    >
                      Open {activePlatform.label} Settings &rarr;
                    </a>
                  )}
                  <p className="dash-platform-help">{activePlatform.help}</p>
                  <div className="dash-platform-config">
                    {!activePlatform.keyOnly && (
                      <input
                        className="dash-input"
                        placeholder="Server URL"
                        value={platformConfig.rtmp_url}
                        onChange={(e) => setPlatformConfig((prev) => ({ ...prev, rtmp_url: e.target.value }))}
                      />
                    )}
                    <input
                      className="dash-input"
                      placeholder="Stream Key (paste from platform)"
                      type="password"
                      value={platformConfig.stream_key}
                      onChange={(e) => setPlatformConfig((prev) => ({ ...prev, stream_key: e.target.value }))}
                    />
                  </div>
                </div>
              )}
            </fieldset>

            {error && <p className="dash-error">{error}</p>}

            <button
              className="record-btn dash-go-live-btn"
              onClick={handleCreateSession}
              disabled={creating}
            >
              {creating ? "Creating stream..." : "Go Live"}
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
                  <span className={`dash-badge dash-badge--${s.status}`}>{s.status}</span>
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
