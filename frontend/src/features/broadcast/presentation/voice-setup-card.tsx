import { Mic, SkipForward, StopCircle } from "lucide-react";
import { cn } from "../../../core/cn";

interface VoiceSetupCardProps {
  elapsedSec: number;
  isRecording: boolean;
  minSec: number;
  maxSec: number;
  onStart: () => void;
  onStop: () => void;
  onSkip: () => void;
}

const SAMPLE_SCRIPT =
  "Welcome to today's live stream! I'm really excited to show you some " +
  "amazing products that I've been using lately. These items have completely " +
  "changed my daily routine, and I think you're going to love them too. The " +
  "quality is outstanding, and the price is incredibly reasonable for what " +
  "you get. I've tried many similar products before, but nothing comes close " +
  "to this. If you have any questions, feel free to drop them in the chat " +
  "and I'll answer them right away. Let's get started!";

function fmtMmSs(sec: number): string {
  const m = Math.floor(sec / 60).toString().padStart(2, "0");
  const s = (sec % 60).toString().padStart(2, "0");
  return `${m}:${s}`;
}

export function VoiceSetupCard({
  elapsedSec,
  isRecording,
  minSec,
  maxSec,
  onStart,
  onStop,
  onSkip,
}: VoiceSetupCardProps) {
  const minReached = elapsedSec >= minSec;
  const denom = minReached ? maxSec : minSec;
  const pct = Math.min(100, Math.round((elapsedSec / denom) * 100));
  const label = minReached
    ? `${fmtMmSs(elapsedSec)} / ${fmtMmSs(maxSec)} max`
    : `${fmtMmSs(elapsedSec)} / ${fmtMmSs(minSec)} (minimum)`;

  return (
    <section className="bg-surface-container-low rounded-xl p-8 max-w-lg mx-auto space-y-6">
      <div>
        <h3 className="font-headline font-bold text-2xl text-on-surface mb-2">
          Voice Setup
        </h3>
        <p className="text-on-surface-variant text-sm leading-relaxed">
          Record at least {minSec} seconds. Longer samples (up to{" "}
          {Math.floor(maxSec / 60)} minutes) produce a noticeably better clone
          and reduce accent bleed in the translated voice.
        </p>
      </div>

      {!isRecording && elapsedSec === 0 && (
        <button
          className="monolith-gradient text-white w-full py-3 rounded-xl font-headline font-bold hover:scale-[0.98] transition-all flex items-center justify-center gap-2"
          onClick={onStart}
        >
          <Mic className="w-5 h-5" />
          Record Voice Sample
        </button>
      )}

      {isRecording && (
        <>
          <div className="flex items-center gap-3 text-error font-label">
            <span className="w-2 h-2 bg-error rounded-full animate-pulse" />
            Recording…
            <span className="ml-auto text-on-surface-variant text-xs tabular-nums">
              {label}
            </span>
          </div>

          <div className="h-2 w-full bg-surface-container-highest rounded-full overflow-hidden">
            <div
              className={cn(
                "h-full transition-all rounded-full",
                minReached ? "bg-success" : "bg-primary",
              )}
              style={{ width: `${pct}%` }}
            />
          </div>

          <div className="bg-surface-container-highest rounded-lg p-4 text-on-surface-variant text-sm leading-relaxed font-body italic">
            {SAMPLE_SCRIPT}
          </div>

          <button
            className={cn(
              "w-full py-3 rounded-xl font-headline font-bold flex items-center justify-center gap-2 transition-all",
              minReached
                ? "bg-success text-white hover:scale-[0.98]"
                : "bg-surface-container-high text-on-surface-variant cursor-not-allowed",
            )}
            disabled={!minReached}
            onClick={onStop}
            title={
              minReached ? "Stop recording and clone voice" : `Wait ${minSec - elapsedSec}s`
            }
          >
            <StopCircle className="w-5 h-5" />
            {minReached ? "Stop & Clone" : `${minSec - elapsedSec}s to minimum`}
          </button>
        </>
      )}

      <button
        className="w-full bg-surface-container-high hover:bg-surface-bright text-on-surface-variant py-2.5 rounded-lg font-label text-sm transition-colors flex items-center justify-center gap-2"
        onClick={onSkip}
      >
        <SkipForward className="w-4 h-4" />
        Skip (use default voice)
      </button>
    </section>
  );
}
