type Props = {
  isLive: boolean;
  canStart: boolean;
  hasRtmpStreams: boolean;
  voiceReady: boolean;
  onStart: () => void;
  onStop: () => void;
  onRtmpRestart: () => void;
};

export function BroadcastControls({ isLive, canStart, hasRtmpStreams, voiceReady, onStart, onStop, onRtmpRestart }: Props) {
  return (
    <div className="flex items-center gap-4">
      {!isLive ? (
        <button
          onClick={onStart}
          disabled={!canStart}
          className="monolith-gradient px-6 py-2.5 rounded-lg text-white font-semibold text-sm disabled:opacity-40 disabled:cursor-not-allowed hover:opacity-90 transition-opacity"
        >
          {hasRtmpStreams ? "Start Broadcasting" : "Start Translation"}
        </button>
      ) : (
        <button
          onClick={onStop}
          className="bg-error-container text-on-error-container px-6 py-2.5 rounded-lg font-semibold text-sm hover:opacity-90 transition-opacity"
        >
          Stop
        </button>
      )}

      {isLive && <LiveIndicator hasRtmp={hasRtmpStreams} voiceReady={voiceReady} onRestart={onRtmpRestart} />}
    </div>
  );
}

function LiveIndicator({ hasRtmp, voiceReady, onRestart }: { hasRtmp: boolean; voiceReady: boolean; onRestart: () => void }) {
  return (
    <div className="flex items-center gap-2">
      <span className="w-2 h-2 rounded-full bg-success animate-pulse" />
      <span className="text-sm text-success font-mono">LIVE</span>
      {hasRtmp && (
        <>
          <span className="text-xs text-primary ml-2">RTMP</span>
          <button
            onClick={onRestart}
            className="ml-2 px-2 py-1 rounded bg-surface-container border border-outline-variant text-on-surface-variant text-xs font-medium hover:bg-surface-container-high transition-colors"
          >
            Restart Stream
          </button>
        </>
      )}
      {voiceReady && <span className="text-xs text-secondary ml-2">Voice cloned</span>}
    </div>
  );
}
