interface TranscriptionDisplayProps {
  transcription: string;
  translation: string;
  durationMs?: number;
}

export function TranscriptionDisplay({ transcription, translation, durationMs }: TranscriptionDisplayProps) {
  if (!transcription && !translation) return null;

  return (
    <div className="results">
      {transcription && (
        <div className="result-card">
          <span className="result-label">Original</span>
          <p className="result-text">{transcription}</p>
        </div>
      )}
      {translation && (
        <div className="result-card translated">
          <span className="result-label">Translation</span>
          <p className="result-text">{translation}</p>
        </div>
      )}
      {durationMs && (
        <div className="latency">
          Pipeline: {durationMs}ms
        </div>
      )}
    </div>
  );
}
