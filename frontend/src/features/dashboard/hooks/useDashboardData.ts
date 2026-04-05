import { useEffect, useState, useCallback } from "react";
import {
  getUser,
  listVoices,
  listSessions,
  listCredentials,
} from "../../../shared/api";
import type {
  UserInfo,
  Voice,
  Session,
  PlatformCredential,
} from "../../../shared/api";

export function useDashboardData(userId: string) {
  const [user, setUser] = useState<UserInfo | null>(null);
  const [voices, setVoices] = useState<Voice[]>([]);
  const [sessions, setSessions] = useState<Session[]>([]);
  const [savedCreds, setSavedCreds] = useState<Record<string, PlatformCredential>>({});
  const [selectedVoice, setSelectedVoice] = useState("");
  const [loading, setLoading] = useState(true);

  const reload = useCallback(async () => {
    try {
      const [u, v, s, c] = await Promise.all([
        getUser(userId),
        listVoices(userId),
        listSessions(userId),
        listCredentials(userId),
      ]);
      setUser(u);
      setVoices(v.voices);
      setSessions(s.sessions);
      buildCredMap(c.credentials);
    } catch (e) {
      console.error("Failed to load data:", e);
    } finally {
      setLoading(false);
    }
  }, [userId]);

  useEffect(() => {
    reload();
  }, [reload]);

  function buildCredMap(credentials: PlatformCredential[]) {
    const map: Record<string, PlatformCredential> = {};
    for (const cred of credentials) map[cred.platform] = cred;
    setSavedCreds(map);
  }

  return {
    user,
    voices,
    sessions,
    savedCreds,
    selectedVoice,
    loading,
    setVoices,
    setSelectedVoice,
    reload,
  };
}
