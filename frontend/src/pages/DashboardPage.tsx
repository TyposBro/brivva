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

  // Session form
  const [title, setTitle] = useState("");
  const [sourceLang, setSourceLang] = useState("ko");
  const [targetLangs, setTargetLangs] = useState<string[]>(["en", "ja", "zh"]);
  const [selectedVoice, setSelectedVoice] = useState<string>("");
  const [creating, setCreating] = useState(false);
  const [error, setError] = useState("");

  // Platforms
  const [enabledPlatforms, setEnabledPlatforms] = useState<Set<string>>(new Set(["youtube"]));
  const [platformConfigs, setPlatformConfigs] = useState<Record<string, PlatformEntry>>({});

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

  const youtubeJustConnected = searchParams.get("youtube") === "connected";

  const toggleLang = (code: string) => {
    setTargetLangs((prev) =>
      prev.includes(code) ? prev.filter((l) => l !== code) : [...prev, code]
    );
  };

  const togglePlatform = (id: string) => {
    setEnabledPlatforms((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  const updatePlatformConfig = (platform: string, field: "rtmp_url" | "stream_key", value: string) => {
    setPlatformConfigs((prev) => ({
      ...prev,
      [platform]: {
        platform,
        rtmp_url: prev[platform]?.rtmp_url ?? "",
        stream_key: prev[platform]?.stream_key ?? "",
        [field]: value,
      },
    }));
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
    if (enabledPlatforms.size === 0) {
      setError("Select at least one platform");
      return;
    }
    if (enabledPlatforms.has("youtube") && !user?.youtube_connected) {
      setError("Connect your YouTube account first, or uncheck YouTube");
      return;
    }

    // Validate manual platforms have RTMP URLs
    for (const pid of enabledPlatforms) {
      const p = api.PLATFORMS.find((x) => x.id === pid);
      if (p && !p.auto) {
        const config = platformConfigs[pid];
        if (!config?.rtmp_url && !p.defaultRtmp) {
          setError(`Enter RTMP URL for ${p.label}`);
          return;
        }
      }
    }

    setCreating(true);
    setError("");

    try {
      const platforms: api.PlatformConfig[] = Array.from(enabledPlatforms).map((pid) => {
        const p = api.PLATFORMS.find((x) => x.id === pid);
        if (p?.auto) return { platform: pid };
        const config = platformConfigs[pid];
        const defaultRtmp = p?.defaultRtmp ?? "";
        return {
          platform: pid,
          rtmp_url: config?.rtmp_url || defaultRtmp,
          stream_key: config?.stream_key ?? "",
        };
      });

      const result = await api.createSession({
        user_id: userId,
        title: title.trim(),
        source_lang: sourceLang,
        target_langs: targetLangs.filter((l) => l !== sourceLang),
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
            <a href={api.youtubeAuthUrl(userId)} className="record-btn dash-yt-connect-btn">
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
              <span onClick={() => navigate("/host")} className="dash-link">host page</span>.
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

            <label className="dash-label">
              Source Language (your language)
              <select
                className="dash-select"
                value={sourceLang}
                onChange={(e) => setSourceLang(e.target.value)}
              >
                {LANGS.map((l) => (
                  <option key={l.code} value={l.code}>{l.label}</option>
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

            {/* Platform Selection */}
            <fieldset className="dash-fieldset">
              <legend className="dash-legend">Stream To</legend>
              <div className="dash-platform-list">
                {["Global", "Korea", "Japan", "China", "Other"].map((region) => {
                  const regionPlatforms = api.PLATFORMS.filter((p) => p.region === region);
                  if (regionPlatforms.length === 0) return null;
                  return (
                    <div key={region} className="dash-platform-region">
                      <div className="dash-platform-region-label">{region}</div>
                      {regionPlatforms.map((p) => {
                        const enabled = enabledPlatforms.has(p.id);
                        const needsConfig = !p.auto && enabled;
                        return (
                          <div key={p.id} className="dash-platform-item">
                            <label className="dash-platform-toggle">
                              <input
                                type="checkbox"
                                checked={enabled}
                                onChange={() => togglePlatform(p.id)}
                              />
                              <span className="dash-platform-label">{p.label}</span>
                              {p.auto && enabled && (
                                <span className="dash-badge dash-badge--success" style={{ marginLeft: "0.5rem" }}>
                                  Auto
                                </span>
                              )}
                            </label>
                            {needsConfig && (
                              <div className="dash-platform-expand">
                                <p className="dash-platform-help">{p.help}</p>
                                <div className="dash-platform-config">
                                  <input
                                    className="dash-input"
                                    placeholder={p.defaultRtmp ? `RTMP URL (pre-filled)` : "RTMP URL (paste from platform)"}
                                    value={platformConfigs[p.id]?.rtmp_url ?? p.defaultRtmp}
                                    onChange={(e) => updatePlatformConfig(p.id, "rtmp_url", e.target.value)}
                                  />
                                  <input
                                    className="dash-input"
                                    placeholder="Stream Key (paste from platform)"
                                    type="password"
                                    value={platformConfigs[p.id]?.stream_key ?? ""}
                                    onChange={(e) => updatePlatformConfig(p.id, "stream_key", e.target.value)}
                                  />
                                </div>
                              </div>
                            )}
                          </div>
                        );
                      })}
                    </div>
                  );
                })}
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
