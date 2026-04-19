import { useEffect, useState } from "react";
import { Loader2, X } from "lucide-react";
import { fetchSessionSummary, type SummaryResponse } from "../data/quote-api";
import * as api from "../data/api-client";

interface Props {
  sessionId: string;
  onClose: () => void;
}

export function SummaryModal({ sessionId, onClose }: Props) {
  const [summary, setSummary] = useState<SummaryResponse | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");

  useEffect(() => {
    let cancelled = false;
    void fetchSessionSummary(sessionId)
      .then((s) => {
        if (!cancelled) setSummary(s);
      })
      .catch((e) => {
        if (!cancelled) setError(e instanceof Error ? e.message : "Summary unavailable");
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [sessionId]);

  return (
    <div className="fixed inset-0 z-[60] bg-background/70 backdrop-blur-sm flex items-center justify-center px-6">
      <div className="bg-surface-container rounded-2xl p-8 max-w-md w-full space-y-6">
        <div className="flex items-start justify-between gap-3">
          <div>
            <h2 className="font-headline font-bold text-2xl text-on-surface">
              Session ended
            </h2>
            <p className="text-on-surface-variant text-sm font-label mt-1">
              Here's what you used. Billing reflects actual minutes per stream.
            </p>
          </div>
          <button
            className="text-on-surface-variant hover:text-on-surface p-1"
            onClick={onClose}
          >
            <X className="w-4 h-4" />
          </button>
        </div>

        <div className="bg-surface-container-low rounded-xl p-4 space-y-3 min-h-[120px]">
          {loading ? (
            <div className="flex items-center gap-2 text-on-surface-variant text-sm font-label">
              <Loader2 className="w-4 h-4 animate-spin text-primary" />
              Loading summary…
            </div>
          ) : summary ? (
            <>
              <div className="grid grid-cols-2 gap-4">
                <div>
                  <span className="text-on-surface-variant text-[10px] font-label uppercase tracking-widest block">
                    Total minutes
                  </span>
                  <span className="text-on-surface font-headline font-bold text-2xl tabular-nums">
                    {summary.totalMinutes}m
                  </span>
                </div>
                <div>
                  <span className="text-on-surface-variant text-[10px] font-label uppercase tracking-widest block">
                    Total cost
                  </span>
                  <span className="text-on-surface font-headline font-bold text-2xl tabular-nums">
                    ${summary.totalCostUsd.toFixed(2)}
                  </span>
                </div>
              </div>

              {summary.breakdown.length > 0 && (
                <div className="space-y-1 pt-2 border-t border-outline-variant/10">
                  {summary.breakdown.map((b) => (
                    <div
                      key={b.lang}
                      className="flex items-center gap-2 text-xs font-label text-on-surface-variant"
                    >
                      <span className="text-on-surface">
                        {api.langFlag(b.lang)} {api.langLabel(b.lang)}
                      </span>
                      <span className="ml-auto tabular-nums">
                        {b.minutes}m · ${b.costUsd.toFixed(2)}
                      </span>
                    </div>
                  ))}
                </div>
              )}
            </>
          ) : (
            <p className="text-on-surface-variant/60 text-sm font-label">
              Summary unavailable: {error || "no data returned"}.
            </p>
          )}
        </div>

        <button
          className="w-full bg-surface-container-high hover:bg-surface-bright text-on-surface px-4 py-3 rounded-xl font-headline font-bold transition-colors"
          onClick={onClose}
        >
          Close
        </button>
      </div>
    </div>
  );
}
