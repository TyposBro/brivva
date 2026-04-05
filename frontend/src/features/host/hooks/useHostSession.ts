import { useState, useCallback } from "react";
import type { Session, StreamInfo } from "../../../shared/api";
import { getSession } from "../../../shared/api";

export function useHostSession() {
  const [session, setSession] = useState<Session | null>(null);
  const [streams, setStreams] = useState<StreamInfo[]>([]);

  const loadSession = useCallback(async (sessionId: string | undefined) => {
    if (!sessionId) return;
    try {
      const data = await getSession(sessionId);
      setSession(data.session);
      setStreams(data.streams);
    } catch (e) {
      console.error("Failed to load session:", e);
    }
  }, []);

  return { session, streams, loadSession };
}
