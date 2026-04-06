import { useState, useCallback, useRef } from "react";

export type DedupError = {
  message: string;
  count: number;
  firstSeen: number;
};

const DEDUP_WINDOW_MS = 5_000;

export function useDeduplicatedErrors() {
  const [errors, setErrors] = useState<DedupError[]>([]);
  const errorsRef = useRef<DedupError[]>([]);

  const addError = useCallback((message: string) => {
    const now = Date.now();
    const existing = errorsRef.current.find(
      (e) => e.message === message && now - e.firstSeen < DEDUP_WINDOW_MS,
    );

    if (existing) {
      existing.count += 1;
      errorsRef.current = [...errorsRef.current];
    } else {
      errorsRef.current = [
        ...errorsRef.current,
        { message, count: 1, firstSeen: now },
      ];
    }

    setErrors([...errorsRef.current]);
  }, []);

  const dismissError = useCallback((index: number) => {
    errorsRef.current = errorsRef.current.filter((_, j) => j !== index);
    setErrors([...errorsRef.current]);
  }, []);

  return { errors, addError, dismissError };
}
