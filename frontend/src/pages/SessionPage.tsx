import { useEffect, useState, useCallback } from "react";
import { useParams, useNavigate } from "react-router-dom";
import {
  ArrowLeft,
  Radio,
  ExternalLink,
  Copy,
  Square,
  Loader2,
} from "lucide-react";
import { cn } from "../lib/cn";
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

  const displayUrl = (url: string) =>
    url
      .replace("rtmp://rtmp:", "rtmp://localhost:")
      .replace("rtmp://server:", "rtmp://localhost:");

  const streamPath = (rtmpUrl: string) =>
    rtmpUrl.replace(/^rtmps?:\/\/[^/]+/, "");

  const getPlatformLabel = (platformId: string) =>
    api.PLATFORMS.find((p) => p.id === platformId)?.label ?? platformId;

  if (loading) {
    return (
      <div className="min-h-screen bg-background flex items-center justify-center">
        <Loader2 className="w-6 h-6 text-primary animate-spin" />
      </div>
    );
  }

  if (!session) {
    return (
      <div className="min-h-screen bg-background flex flex-col items-center justify-center gap-4">
        <p className="text-error font-label">Session not found</p>
        <button
          className="bg-surface-container-high hover:bg-surface-bright text-on-surface px-5 py-2.5 rounded-lg font-headline font-bold transition-colors"
          onClick={() => navigate("/dashboard")}
        >
          Back to Dashboard
        </button>
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
    <div className="min-h-screen bg-background">
      {/* Header */}
      <header className="fixed top-0 w-full z-50 bg-background/60 backdrop-blur-xl">
        <div className="flex justify-between items-center max-w-5xl mx-auto px-8 h-16">
          <button
            className="flex items-center gap-2 text-on-surface-variant hover:text-on-surface transition-colors font-label text-sm"
            onClick={() => navigate("/dashboard")}
          >
            <ArrowLeft className="w-4 h-4" />
            Dashboard
          </button>
          <h1 className="text-xl font-bold tracking-tighter text-on-surface font-headline">
            BRIVVA
          </h1>
          <p className="text-on-surface-variant font-label text-sm uppercase tracking-widest">
            Session
          </p>
        </div>
      </header>

      <main className="max-w-5xl mx-auto px-8 pt-24 pb-16 space-y-12">
        {/* Session Info */}
        <section>
          <div className="flex items-center gap-4 mb-2">
            <h2 className="font-headline font-bold text-3xl tracking-tight text-on-surface">
              {session.title}
            </h2>
            <span
              className={cn(
                "text-[10px] font-label font-bold uppercase tracking-widest px-2 py-0.5 rounded",
                session.status === "live"
                  ? "text-success bg-success/10"
                  : session.status === "ended"
                    ? "text-on-surface-variant bg-surface-container-highest"
                    : "text-primary bg-primary/10"
              )}
            >
              {session.status}
            </span>
          </div>
          <div className="flex gap-4 text-on-surface-variant text-sm font-label">
            <span>
              Source:{" "}
              <span className="text-on-surface">
                {api.langLabel(session.source_lang)}
              </span>
            </span>
            <span>
              Targets:{" "}
              <span className="text-on-surface">
                {targetLangs.map((l) => api.langLabel(l)).join(", ")}
              </span>
            </span>
          </div>
        </section>

        {/* Stream Cards */}
        <section>
          <h3 className="font-headline font-bold text-xl tracking-tight text-on-surface mb-4">
            Streams
          </h3>
          <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
            {streams.map((s) => (
              <div
                key={s.id}
                className="bg-surface-container-low rounded-xl p-5 space-y-3"
              >
                {/* Header */}
                <div className="flex items-center gap-3">
                  <span className="text-[10px] font-label font-bold uppercase tracking-widest text-primary bg-primary/10 px-2 py-0.5 rounded">
                    {s.lang?.toUpperCase()}
                  </span>
                  {s.platform && (
                    <span className="text-on-surface font-label text-sm">
                      {getPlatformLabel(s.platform)}
                    </span>
                  )}
                  <span
                    className={cn(
                      "ml-auto text-[10px] font-label font-bold uppercase tracking-widest px-2 py-0.5 rounded",
                      s.error
                        ? "text-error bg-error-container/30"
                        : s.status === "ready"
                          ? "text-success bg-success/10"
                          : "text-on-surface-variant bg-surface-container-highest"
                    )}
                  >
                    {s.error ? "Error" : s.status ?? "pending"}
                  </span>
                </div>

                {s.error && (
                  <p className="text-error text-xs font-label">{s.error}</p>
                )}

                {/* URLs */}
                <div className="space-y-1.5">
                  {s.broadcast_id && (
                    <a
                      href={`https://youtube.com/watch?v=${s.broadcast_id}`}
                      target="_blank"
                      rel="noopener noreferrer"
                      className="flex items-center gap-1.5 text-primary text-xs font-label hover:underline"
                    >
                      <ExternalLink className="w-3 h-3" />
                      youtube.com/watch?v={s.broadcast_id}
                    </a>
                  )}

                  {s.rtmp_url && (
                    <div className="flex items-center gap-2">
                      <code className="text-on-surface-variant text-xs font-mono truncate flex-1">
                        {displayUrl(s.rtmp_url)}
                      </code>
                      <button
                        className="text-on-surface-variant hover:text-primary transition-colors p-0.5"
                        onClick={() =>
                          navigator.clipboard.writeText(displayUrl(s.rtmp_url!))
                        }
                      >
                        <Copy className="w-3 h-3" />
                      </button>
                    </div>
                  )}

                  {s.platform === "local-test" && s.rtmp_url && (
                    <>
                      <a
                        href={`http://localhost:8889${streamPath(s.rtmp_url)}/`}
                        target="_blank"
                        rel="noopener noreferrer"
                        className="flex items-center gap-1.5 text-primary text-xs font-label hover:underline"
                      >
                        <ExternalLink className="w-3 h-3" />
                        WebRTC: localhost:8889{streamPath(s.rtmp_url)}/
                      </a>
                      <a
                        href={`http://localhost:8888${streamPath(s.rtmp_url)}/`}
                        target="_blank"
                        rel="noopener noreferrer"
                        className="flex items-center gap-1.5 text-primary text-xs font-label hover:underline"
                      >
                        <ExternalLink className="w-3 h-3" />
                        HLS: localhost:8888{streamPath(s.rtmp_url)}/
                      </a>
                    </>
                  )}
                </div>
              </div>
            ))}
          </div>
        </section>

        {/* Actions */}
        <section className="flex flex-col sm:flex-row gap-3">
          {isLive && !session.room_id && (
            <button
              className="monolith-gradient text-white px-8 py-3 rounded-xl font-headline font-extrabold hover:scale-[0.98] transition-all shadow-xl flex items-center justify-center gap-2"
              onClick={() =>
                navigate(
                  `/host?sessionId=${session.id}&sourceLang=${session.source_lang}`
                )
              }
            >
              <Radio className="w-5 h-5" />
              Start Broadcasting
            </button>
          )}

          {isLive && session.room_id && (
            <div className="flex items-center gap-3 bg-surface-container-low px-5 py-3 rounded-xl">
              <span className="text-on-surface-variant text-sm font-label">
                Room Code:
              </span>
              <code className="text-primary font-mono font-bold text-lg">
                {session.room_id}
              </code>
              <button
                className="text-on-surface-variant hover:text-primary transition-colors"
                onClick={() =>
                  navigator.clipboard.writeText(session.room_id ?? "")
                }
              >
                <Copy className="w-4 h-4" />
              </button>
            </div>
          )}

          {isLive && (
            <button
              className="bg-error-container text-on-error-container px-6 py-3 rounded-xl font-headline font-bold hover:opacity-90 transition-opacity flex items-center justify-center gap-2"
              onClick={handleEnd}
              disabled={ending}
            >
              <Square className="w-4 h-4" />
              {ending ? "Ending..." : "End Session"}
            </button>
          )}

          <button
            className="bg-surface-container-high hover:bg-surface-bright text-on-surface px-6 py-3 rounded-xl font-headline font-bold transition-colors flex items-center justify-center gap-2"
            onClick={() => navigate("/dashboard")}
          >
            <ArrowLeft className="w-4 h-4" />
            Back to Dashboard
          </button>
        </section>
      </main>
    </div>
  );
}
