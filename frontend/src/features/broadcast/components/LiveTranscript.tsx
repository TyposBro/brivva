import { useRef, useEffect } from "react";
import { LANGS, type TranscriptEntry } from "../constants";

type Props = {
  transcripts: TranscriptEntry[];
  interim: string;
};

export function LiveTranscript({ transcripts, interim }: Props) {
  const scrollRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const el = scrollRef.current;
    el?.scrollTo({ top: el.scrollHeight, behavior: "smooth" });
  }, [transcripts, interim]);

  const isEmpty = transcripts.length === 0 && !interim;

  return (
    <div
      ref={scrollRef}
      className="bg-surface-container-lowest rounded-xl p-4 space-y-3 max-h-96 overflow-y-auto"
    >
      <h2 className="font-headline text-sm font-semibold text-on-surface-variant uppercase tracking-wider sticky top-0 bg-surface-container-lowest pb-2">
        Live Transcript
      </h2>

      {transcripts.map((entry) => (
        <TranscriptRow key={entry.id} entry={entry} />
      ))}

      {interim && <div className="text-sm text-outline italic">{interim}</div>}

      {isEmpty && (
        <div className="text-sm text-outline text-center py-4">
          Listening... speak into your microphone
        </div>
      )}
    </div>
  );
}

function TranscriptRow({ entry }: { entry: TranscriptEntry }) {
  return (
    <div className="space-y-1">
      <div className="text-sm text-on-surface">
        <span className="text-outline text-xs font-mono mr-2">#{entry.id}</span>
        {entry.text}
      </div>
      {Object.entries(entry.translations).map(([lang, text]) => (
        <div key={lang} className="text-sm text-on-surface-variant pl-6">
          <span className="text-xs font-mono text-secondary mr-1">
            {LANGS.find((l) => l.code === lang)?.flag}
          </span>
          {text}
        </div>
      ))}
    </div>
  );
}
