import { useEffect, useState, useCallback, type ReactNode } from "react";
import { useParams, useNavigate } from "react-router-dom";
import {
  ArrowLeft,
  Radio,
  ExternalLink,
  Copy,
  Square,
  Loader2,
  Activity,
  DollarSign,
  Clock,
} from "lucide-react";
import { cn } from "../../../core/cn";
import * as api from "../data/api-client";
import { SummaryModal } from "./summary-modal";

const POLL_INTERVAL_MS = 5_000;
const TICK_INTERVAL_MS = 30_000;
const COST_PER_OUTPUT_MINUTE_USD = 0.10;

export default function SessionPage() {
  const { id } = useParams<{ id: string }>();
  const navigate = useNavigate();

  const [session, setSession] = useState<api.Session | null>(null);
  const [streams, setStreams] = useState<api.StreamInfo[]>([]);
  const [loading, setLoading] = useState(true);
  const [ending, setEnding] = useState(false);
  const [now, setNow] = useState(() => Date.now());
  const [showSummary, setShowSummary] = useState(false);

  const loadSession = useCallback(async () => {
    if (!id) return;
    try {
      const data = await api.getSession(id);
      // Don't clobber an already-loaded session with null. A null response on
      // a subsequent refresh is almost always a race (Workers soft-ends the
      // row, an in-flight GET that started before End-Session can return
      // session:null on D1 read-after-write), and falling back to "Session
      // not found" right after the host clicked End was the prod incident
      // 2026-04-20 we are fixing. The `!session` fallback is only meaningful
      // on the FIRST load — once we have a row we keep it.
      setSession((prev) => data.session ?? prev);
      setStreams(data.streams);
    } catch (e) {
      console.error("Failed to load session:", e);
    } finally {
      setLoading(false);
    }
  }, [id]);

  useEffect(() => {
    void loadSession();
  }, [loadSession]);

  // Poll session state + tick the live-minutes clock so the cost estimate
  // and minutes counter stay current without requiring a manual refresh.
  useEffect(() => {
    if (session?.status !== "live") return;
    const poll = setInterval(() => void loadSession(), POLL_INTERVAL_MS);
    const tick = setInterval(() => setNow(Date.now()), TICK_INTERVAL_MS);
    return () => {
      clearInterval(poll);
      clearInterval(tick);
    };
  }, [session?.status, loadSession]);

  const handleEnd = async () => {
    if (!id || ending) return;
    setEnding(true);
    try {
      await api.deleteSession(id);
      // Optimistically flip status BEFORE the refresh round-trip. Workers
      // soft-ends the row (status='ended') so the GET below returns the same
      // session, but if that GET races / errors we still want the host to
      // see the summary modal — not a "Session not found" screen — because
      // the End was already accepted. Prod incident 2026-04-20: the row used
      // to be hard-deleted, GET returned `session:null`, and the page fell
      // through to its `!session` branch under the modal.
      setSession((prev) => (prev ? { ...prev, status: "ended" } : prev));
      setShowSummary(true);
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

  const observability = computeObservability({ session, streams, isLive, nowMs: now });

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

        {/* Observability */}
        <ObservabilityPanel
          observability={observability}
          targetStreamCount={streams.filter((s) => s.lang !== session.source_lang).length}
        />

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

                {/* Bytes pushed — TODO(server-rs agent): expose
                    GET /metrics/streams/:id with bytes_in/bytes_out so we can
                    render real counters here instead of the dash. */}
                <div className="flex items-center gap-2 text-on-surface-variant text-[11px] font-label">
                  <Activity className="w-3 h-3" />
                  <span>Bytes pushed:</span>
                  <span className="font-mono text-on-surface">
                    {observability.streamBytes[s.id] ?? "—"}
                  </span>
                </div>

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
          {isLive && (
            <button
              className="monolith-gradient text-white px-8 py-3 rounded-xl font-headline font-extrabold hover:scale-[0.98] transition-all shadow-xl flex items-center justify-center gap-2"
              onClick={() => {
                const target = session.voice_id
                  ? `/session/${session.id}/live`
                  : `/session/${session.id}/setup`;
                navigate(target);
              }}
            >
              <Radio className="w-5 h-5" />
              {session.live_session_id ? "Resume Broadcast" : "Start Broadcasting"}
            </button>
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

      {showSummary && id && (
        <SummaryModal sessionId={id} onClose={() => setShowSummary(false)} />
      )}
    </div>
  );
}

interface Observability {
  liveMinutes: number;
  estimatedCostUsd: number;
  streamBytes: Record<string, string>;
  isApproximated: boolean;
}

interface ComputeArgs {
  session: api.Session;
  streams: api.StreamInfo[];
  isLive: boolean;
  nowMs: number;
}

function computeObservability({ session, streams, isLive, nowMs }: ComputeArgs): Observability {
  // PRAGMATIC: session.created_at is the closest signal we have to a stream
  // start. There is no started_at column on the session row yet — once
  // server-rs surfaces real per-stream start timestamps via /metrics/streams,
  // swap this proxy for the authoritative value.
  const startMs = (session.created_at ?? 0) * 1000;
  const elapsedMs = isLive && startMs > 0 ? Math.max(0, nowMs - startMs) : 0;
  const liveMinutes = Math.floor(elapsedMs / 60_000);

  const targetStreams = streams.filter((s) => s.lang !== session.source_lang);
  const estimatedCostUsd = liveMinutes * targetStreams.length * COST_PER_OUTPUT_MINUTE_USD;

  // TODO(server-rs agent): expose GET /metrics/streams that returns
  // bytes_in/bytes_out per stream id. Until then we render "—" for every row.
  const streamBytes: Record<string, string> = {};

  return { liveMinutes, estimatedCostUsd, streamBytes, isApproximated: true };
}

function ObservabilityPanel({
  observability,
  targetStreamCount,
}: {
  observability: Observability;
  targetStreamCount: number;
}) {
  const { liveMinutes, estimatedCostUsd, isApproximated } = observability;
  return (
    <section className="grid grid-cols-1 sm:grid-cols-3 gap-3">
      <Stat
        icon={<Clock className="w-4 h-4" />}
        label="Live minutes"
        value={`${liveMinutes}m`}
        hint={isApproximated ? "from session start" : undefined}
      />
      <Stat
        icon={<DollarSign className="w-4 h-4" />}
        label="Estimated cost"
        value={`$${estimatedCostUsd.toFixed(2)}`}
        hint={`${targetStreamCount} target × $${COST_PER_OUTPUT_MINUTE_USD.toFixed(2)}/min`}
      />
      <Stat
        icon={<Activity className="w-4 h-4" />}
        label="Per-stream bytes"
        value="—"
        hint="awaiting server-rs metrics"
      />
    </section>
  );
}

function Stat({
  icon,
  label,
  value,
  hint,
}: {
  icon: ReactNode;
  label: string;
  value: string;
  hint?: string;
}) {
  return (
    <div className="bg-surface-container-low rounded-xl p-4 space-y-1.5">
      <div className="flex items-center gap-2 text-on-surface-variant text-xs font-label uppercase tracking-widest">
        {icon}
        <span>{label}</span>
      </div>
      <p className="text-on-surface font-headline font-bold text-2xl tabular-nums">
        {value}
      </p>
      {hint && (
        <p className="text-on-surface-variant/60 text-[11px] font-label">{hint}</p>
      )}
    </div>
  );
}
