import { useEffect, useState } from "react";
import { Loader2, X } from "lucide-react";
import { fetchSessionQuote, type QuoteResponse } from "../data/quote-api";
import * as api from "../data/api-client";

const MIN_MINUTES = 10;
const MAX_MINUTES = 180;
const DEFAULT_MINUTES = 30;

interface Props {
  sessionId: string;
  onConfirm: () => void;
  onCancel: () => void;
}

export function QuoteModal({ sessionId, onConfirm, onCancel }: Props) {
  const [minutes, setMinutes] = useState(DEFAULT_MINUTES);
  const [quote, setQuote] = useState<QuoteResponse | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError("");
    void fetchSessionQuote(sessionId, minutes)
      .then((q) => {
        if (cancelled) return;
        setQuote(q);
      })
      .catch((e) => {
        if (cancelled) return;
        // Workers may not have shipped /quote yet — fall back to local
        // estimate so the user still sees a number.
        setError(e instanceof Error ? e.message : "Quote unavailable");
        setQuote(null);
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [sessionId, minutes]);

  return (
    <div className="fixed inset-0 z-[60] bg-background/70 backdrop-blur-sm flex items-center justify-center px-6">
      <div className="bg-surface-container rounded-2xl p-8 max-w-md w-full space-y-6">
        <div className="flex items-start justify-between gap-3">
          <div>
            <h2 className="font-headline font-bold text-2xl text-on-surface">
              How long will you stream?
            </h2>
            <p className="text-on-surface-variant text-sm font-label mt-1">
              Cost is billed per output minute. Your actual bill matches the
              real session length.
            </p>
          </div>
          <button
            className="text-on-surface-variant hover:text-on-surface p-1"
            onClick={onCancel}
          >
            <X className="w-4 h-4" />
          </button>
        </div>

        <div className="space-y-2">
          <div className="flex items-baseline justify-between">
            <span className="text-on-surface-variant text-xs font-label uppercase tracking-widest">
              Expected duration
            </span>
            <span className="text-on-surface font-headline font-bold text-lg tabular-nums">
              {minutes} min
            </span>
          </div>
          <input
            type="range"
            min={MIN_MINUTES}
            max={MAX_MINUTES}
            step={5}
            value={minutes}
            onChange={(e) => setMinutes(Number(e.target.value))}
            className="w-full accent-primary"
          />
          <div className="flex justify-between text-on-surface-variant/60 text-[10px] font-label">
            <span>{MIN_MINUTES} min</span>
            <span>{MAX_MINUTES} min</span>
          </div>
        </div>

        <div className="bg-surface-container-low rounded-xl p-4 space-y-2 min-h-[88px]">
          {loading ? (
            <div className="flex items-center gap-2 text-on-surface-variant text-sm font-label">
              <Loader2 className="w-4 h-4 animate-spin text-primary" />
              Calculating estimate…
            </div>
          ) : quote ? (
            <>
              <div className="flex items-baseline justify-between">
                <span className="text-on-surface-variant text-xs font-label uppercase tracking-widest">
                  Estimated cost
                </span>
                <span className="text-on-surface font-headline font-bold text-2xl tabular-nums">
                  ${quote.estimatedCostUsd.toFixed(2)}
                </span>
              </div>
              {quote.breakdown.length > 0 && (
                <div className="space-y-1 pt-1 border-t border-outline-variant/10">
                  {quote.breakdown.map((b) => (
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
              Estimate unavailable: {error || "no quote returned"}.
              You can still continue — billing reflects actual usage.
            </p>
          )}
        </div>

        <div className="flex gap-3">
          <button
            className="flex-1 bg-surface-container-high hover:bg-surface-bright text-on-surface px-4 py-3 rounded-xl font-headline font-bold transition-colors"
            onClick={onCancel}
          >
            Not yet
          </button>
          <button
            className="flex-1 monolith-gradient text-white px-4 py-3 rounded-xl font-headline font-bold hover:scale-[0.98] transition-all"
            onClick={onConfirm}
          >
            Continue
          </button>
        </div>
      </div>
    </div>
  );
}
