import { useEffect, useState, useCallback } from "react";
import type { Session, StreamInfo } from "../../../shared/api";
import { getSession, deleteSession } from "../../../shared/api";

export function useSessionData(id: string | undefined) {
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

  const endSession = useCallback(async () => {
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
  }, [id, ending, loadSession]);

  return { session, streams, loading, ending, endSession };
}
