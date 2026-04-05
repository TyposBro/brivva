import { useEffect, useState, useCallback } from "react";
import { useParams, useNavigate } from "react-router-dom";
import { Loader2 } from "lucide-react";
import type { Session, StreamInfo } from "../../shared/api";
import { getSession, deleteSession } from "../../shared/api";
import { PageLayout } from "../../shared/components/PageLayout";
import { BackButton } from "../../shared/components/BackButton";
import { parseTargetLangs } from "./utils/parseTargetLangs";
import { StreamCard } from "./components/StreamCard";
import { SessionInfo } from "./components/SessionInfo";
import { SessionActions } from "./components/SessionActions";

export default function SessionPage() {
  const { id } = useParams<{ id: string }>();
  const navigate = useNavigate();

  const [session, setSession] = useState<Session | null>(null);
  const [streams, setStreams] = useState<StreamInfo[]>([]);
  const [loading, setLoading] = useState(true);
  const [ending, setEnding] = useState(false);

  const loadSession = useCallback(async () => {
    if (!id) return;
    try {
      const data = await getSession(id);
      setSession(data.session);
      setStreams(data.streams);
    } catch (e) {
      console.error("Failed to load session:", e);
    } finally {
      setLoading(false);
    }
  }, [id]);

  useEffect(() => { loadSession(); }, [loadSession]);

  const handleEnd = async () => {
    if (!id || ending) return;
    setEnding(true);
    try {
      await deleteSession(id);
      await loadSession();
    } catch (e) {
      console.error("Failed to end session:", e);
    } finally {
      setEnding(false);
    }
  };

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

  const targetLangs = parseTargetLangs(session.target_langs);
  const isLive = session.status === "live";

  return (
    <PageLayout
      left={<BackButton label="Dashboard" onClick={() => navigate("/dashboard")} />}
      right={
        <p className="text-on-surface-variant font-label text-sm uppercase tracking-widest">
          Session
        </p>
      }
      spaceY={12}
    >
      <SessionInfo session={session} targetLangs={targetLangs} />

      <section>
        <h3 className="font-headline font-bold text-xl tracking-tight text-on-surface mb-4">
          Streams
        </h3>
        <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
          {streams.map((s) => (
            <StreamCard key={s.id} stream={s} />
          ))}
        </div>
      </section>

      <SessionActions
        session={session}
        isLive={isLive}
        ending={ending}
        onEnd={handleEnd}
        onNavigate={navigate}
      />
    </PageLayout>
  );
}
