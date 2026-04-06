import { useState, useCallback, useRef } from "react";
import type { PipelineHealthSnapshot } from "../../domain/health-types";

const HIGH_DRIFT_DURATION_MS = 10_000;

export function useDriftWarning(broadcastDelay: number) {
  const [showDriftWarning, setShowDriftWarning] = useState(false);
  const highDriftSinceRef = useRef<number | null>(null);
  const driftThreshold = broadcastDelay * 2;

  const checkDrift = useCallback((snapshot: PipelineHealthSnapshot | null) => {
    if (!snapshot || snapshot.languages.length === 0) {
      highDriftSinceRef.current = null;
      setShowDriftWarning(false);
      return;
    }

    const anyHighDrift = snapshot.languages.some(
      (lang) => lang.driftMs > driftThreshold,
    );

    if (!anyHighDrift) {
      highDriftSinceRef.current = null;
      setShowDriftWarning(false);
      return;
    }

    const now = Date.now();
    if (highDriftSinceRef.current === null) {
      highDriftSinceRef.current = now;
      return;
    }

    if (now - highDriftSinceRef.current > HIGH_DRIFT_DURATION_MS) {
      setShowDriftWarning(true);
    }
  }, [driftThreshold]);

  const dismissDriftWarning = useCallback(() => {
    setShowDriftWarning(false);
    highDriftSinceRef.current = null;
  }, []);

  return { showDriftWarning, checkDrift, dismissDriftWarning };
}
