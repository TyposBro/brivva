import { useRef, useEffect } from "react";
import { LANG_LABELS, type Lang } from "../../../types";
import type { GuestUtterance } from "../state/reducer";

type Props = {
  utterances: GuestUtterance[];
  liveTranscript: string;
  selectedLang: Lang;
};

export function GuestTranscripts({ utterances, liveTranscript, selectedLang }: Props) {
  const listRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (listRef.current) listRef.current.scrollTop = listRef.current.scrollHeight;
  }, [utterances, liveTranscript]);

  return (
    <div className="utterances" ref={listRef}>
      {utterances.map((u) => (
        <div key={u.id} className="utterance">
          <div className="result-card">
            <span className="result-label">Korean</span>
            <p className="result-text">{u.original}</p>
          </div>
          {u.translation ? (
            <div className="result-card translated">
              <span className="result-label">{LANG_LABELS[selectedLang]}</span>
              <p className="result-text">{u.translation}</p>
            </div>
          ) : (
            <div className="result-card translating">
              <span className="spinner" />
            </div>
          )}
        </div>
      ))}

      {liveTranscript && (
        <div className="utterance live">
          <div className="result-card live-card">
            <span className="result-label">
              <span className="dot listening-dot" /> Live
            </span>
            <p className="result-text">{liveTranscript}</p>
          </div>
        </div>
      )}
    </div>
  );
}
