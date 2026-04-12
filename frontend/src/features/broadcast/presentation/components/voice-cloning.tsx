import { CLONE_MIN_DURATION_SEC, CLONE_MAX_DURATION_SEC } from "../../domain/broadcast-constants";
import type { ClonePhase } from "../hooks/use-voice-clone";

type Props = {
  phase: ClonePhase;
  elapsedSec: number;
  isMinReached: boolean;
  voiceReady: boolean;
  onClone: () => void;
  onStop: () => void;
};

const SAMPLE_TEXT =
  '"The sun went down and the sky turned orange and purple. Below, the city lights began ' +
  "to flicker on. The busy noise of the day, cars honking and people rushing, started to " +
  "fade away. It was finally quiet. While most people were going home to eat dinner and " +
  'rest, some were just starting their work."';

export function VoiceCloning({ phase, elapsedSec, isMinReached, voiceReady, onClone, onStop }: Props) {
  return (
    <div className="bg-surface-container-low rounded-xl p-4 space-y-3">
      <div className="flex items-center justify-between">
        <h2 className="font-headline text-sm font-semibold text-on-surface-variant uppercase tracking-wider">
          Voice Cloning
        </h2>
        {voiceReady && (
          <span className="text-xs text-secondary font-medium px-2 py-0.5 bg-secondary-container rounded-full">
            Active
          </span>
        )}
      </div>

      {phase === "idle"
        ? <CloningPrompt voiceReady={voiceReady} onClone={onClone} />
        : <CloningProgress phase={phase} elapsedSec={elapsedSec} isMinReached={isMinReached} onStop={onStop} />}
    </div>
  );
}

function formatTime(sec: number): string {
  const m = Math.floor(sec / 60);
  const s = sec % 60;
  return `${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`;
}

function CloningProgress({ phase, elapsedSec, isMinReached, onStop }: {
  phase: ClonePhase;
  elapsedSec: number;
  isMinReached: boolean;
  onStop: () => void;
}) {
  const isUploading = phase === "uploading";

  return (
    <div className="space-y-3">
      <div className="bg-surface-container rounded-lg p-3">
        <p className="text-sm text-on-surface leading-relaxed italic">{SAMPLE_TEXT}</p>
      </div>

      <ProgressBar elapsedSec={elapsedSec} />

      <div className="flex items-center justify-between">
        <TimerLabel elapsedSec={elapsedSec} isMinReached={isMinReached} isUploading={isUploading} />
        {isMinReached && !isUploading && (
          <button
            onClick={onStop}
            className="px-4 py-1.5 rounded-lg bg-error-container text-on-error-container text-sm font-semibold hover:opacity-90 transition-opacity"
          >
            Stop Recording
          </button>
        )}
      </div>

      {!isUploading && (
        <p className="text-xs text-outline">
          Read the text above naturally into your microphone.
        </p>
      )}
    </div>
  );
}

function ProgressBar({ elapsedSec }: { elapsedSec: number }) {
  const PERCENT = 100;
  const progress = Math.min(elapsedSec / CLONE_MAX_DURATION_SEC, 1);
  const minMarkerPct = (CLONE_MIN_DURATION_SEC / CLONE_MAX_DURATION_SEC) * PERCENT;

  return (
    <div className="relative h-2 bg-surface-container-high rounded-full overflow-hidden">
      <div
        className="h-full bg-secondary rounded-full transition-all duration-200"
        style={{ width: `${progress * PERCENT}%` }}
      />
      <div
        className="absolute top-0 h-full w-0.5 bg-on-surface-variant opacity-50"
        style={{ left: `${minMarkerPct}%` }}
      />
    </div>
  );
}

function TimerLabel({ elapsedSec, isMinReached, isUploading }: {
  elapsedSec: number;
  isMinReached: boolean;
  isUploading: boolean;
}) {
  if (isUploading) {
    return <span className="text-sm text-on-surface-variant font-mono">Uploading...</span>;
  }

  return (
    <span className="text-sm text-on-surface-variant font-mono">
      {formatTime(elapsedSec)}
      {isMinReached
        ? <span className="ml-2 text-secondary">&#10003; Minimum reached</span>
        : <span className="ml-2 text-outline">min {CLONE_MIN_DURATION_SEC}s</span>}
    </span>
  );
}

function CloningPrompt({ voiceReady, onClone }: {
  voiceReady: boolean;
  onClone: () => void;
}) {
  return (
    <div className="space-y-2">
      <p className="text-xs text-outline">
        {voiceReady
          ? "Your cloned voice will be used for all translated speech."
          : "Record your voice (30s\u20133min). All translations will use your cloned voice instead of the default."}
      </p>
      <button
        onClick={onClone}
        className="px-4 py-2 rounded-lg bg-secondary-container text-on-secondary-container text-sm font-semibold hover:opacity-90 transition-opacity"
      >
        {voiceReady ? "Re-clone Voice" : "Start Recording"}
      </button>
    </div>
  );
}
