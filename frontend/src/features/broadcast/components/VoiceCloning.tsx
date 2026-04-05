import { CLONE_DURATION_SEC } from "../constants";

type Props = {
  isCloning: boolean;
  cloneProgress: number;
  voiceReady: boolean;
  onClone: () => void;
};

const SAMPLE_TEXT =
  '"The sun went down and the sky turned orange and purple. Below, the city lights began ' +
  "to flicker on. The busy noise of the day, cars honking and people rushing, started to " +
  "fade away. It was finally quiet. While most people were going home to eat dinner and " +
  'rest, some were just starting their work."';

export function VoiceCloning({ isCloning, cloneProgress, voiceReady, onClone }: Props) {
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

      {isCloning
        ? <CloningProgress progress={cloneProgress} />
        : <CloningPrompt voiceReady={voiceReady} onClone={onClone} />}
    </div>
  );
}

const PERCENT = 100;

function CloningProgress({ progress }: { progress: number }) {
  const elapsed = Math.round(progress * CLONE_DURATION_SEC);

  return (
    <div className="space-y-3">
      <div className="bg-surface-container rounded-lg p-3">
        <p className="text-sm text-on-surface leading-relaxed italic">{SAMPLE_TEXT}</p>
      </div>
      <div className="flex items-center gap-3">
        <div className="flex-1 h-2 bg-surface-container-high rounded-full overflow-hidden">
          <div
            className="h-full bg-secondary rounded-full transition-all duration-200"
            style={{ width: `${progress * PERCENT}%` }}
          />
        </div>
        <span className="text-sm text-on-surface-variant font-mono w-24 text-right">
          {progress < 1 ? `${elapsed}s / ${CLONE_DURATION_SEC}s` : "Uploading..."}
        </span>
      </div>
      {progress < 1 && (
        <p className="text-xs text-outline">
          Read the text above naturally into your microphone.
        </p>
      )}
    </div>
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
          : `Record ${CLONE_DURATION_SEC} seconds of your voice. All translations will use your cloned voice instead of the default.`}
      </p>
      <button
        onClick={onClone}
        className="px-4 py-2 rounded-lg bg-secondary-container text-on-secondary-container text-sm font-semibold hover:opacity-90 transition-opacity"
      >
        {voiceReady ? "Re-clone Voice" : `Start Recording (${CLONE_DURATION_SEC}s)`}
      </button>
    </div>
  );
}
