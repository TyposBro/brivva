import { useState, useEffect, useRef, useCallback } from "react";
import type { DubbingJob } from "../../domain/types";
import { getSessionJobs } from "../../data/api";

const POLL_INTERVAL_MS = 5_000;

export function useDubbingJobs(sessionId: string | undefined) {
  const [jobs, setJobs] = useState<DubbingJob[]>([]);
  const timerRef = useRef<ReturnType<typeof setInterval> | null>(null);

  const fetchJobs = useCallback(async () => {
    if (!sessionId) return;
    try {
      const data = await getSessionJobs(sessionId);
      setJobs(data);
    } catch {
      // Silently ignore poll failures
    }
  }, [sessionId]);

  useEffect(() => {
    fetchJobs();
  }, [fetchJobs]);

  useEffect(() => {
    const hasActive = jobs.some(
      (j) => j.status !== "complete" && j.status !== "failed",
    );
    if (!hasActive) {
      if (timerRef.current) clearInterval(timerRef.current);
      timerRef.current = null;
      return;
    }
    if (timerRef.current) return;
    timerRef.current = setInterval(fetchJobs, POLL_INTERVAL_MS);
    return () => { if (timerRef.current) clearInterval(timerRef.current); };
  }, [jobs, fetchJobs]);

  return { jobs, refetch: fetchJobs };
}
