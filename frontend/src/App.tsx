import { useState, useCallback } from "react";
import { useAudioRecorder } from "./hooks/useAudioRecorder";
import { AudioRecorder } from "./components/AudioRecorder";
import { TranscriptionDisplay } from "./components/TranscriptionDisplay";
import { AudioPlayer } from "./components/AudioPlayer";

const LANGUAGES = [
  { code: "ko", label: "Korean" },
  { code: "en", label: "English" },
  { code: "ja", label: "Japanese" },
  { code: "zh", label: "Chinese" },
  { code: "es", label: "Spanish" },
  { code: "fr", label: "French" },
  { code: "de", label: "German" },
];

type Result = {
  transcription: string;
  translation: string;
  sourceLang: string;
  targetLang: string;
  durationMs: number;
};

const WORKER_URL = import.meta.env.VITE_WORKER_URL ?? "http://localhost:8787";

export default function App() {
  const [sourceLang, setSourceLang] = useState("ko");
  const [targetLang, setTargetLang] = useState("en");
  const [result, setResult] = useState<Result | null>(null);
  const [error, setError] = useState<string | null>(null);

  const handleAudioReady = useCallback(
    async (blob: Blob) => {
      setError(null);
      setResult(null);

      const formData = new FormData();
      formData.append("audio", blob, "recording.webm");
      formData.append("sourceLang", sourceLang);
      formData.append("targetLang", targetLang);

      try {
        const res = await fetch(`${WORKER_URL}/api/translate`, {
          method: "POST",
          body: formData,
        });

        if (!res.ok) {
          const err = await res.json().catch(() => ({ message: "Unknown error" }));
          throw new Error((err as { message: string }).message);
        }

        const data = (await res.json()) as Result;
        setResult(data);
      } catch (err) {
        setError(err instanceof Error ? err.message : "Something went wrong");
      } finally {
        recorderControls.reset();
      }
    },
    [sourceLang, targetLang]
  );

  const recorderControls = useAudioRecorder(handleAudioReady);

  const swapLanguages = () => {
    setSourceLang(targetLang);
    setTargetLang(sourceLang);
    setResult(null);
  };

  return (
    <div className="app">
      <header className="header">
        <h1 className="logo">brivva</h1>
        <p className="tagline">Real-time voice translation</p>
      </header>

      <main className="main">
        <div className="lang-row">
          <select
            className="lang-select"
            value={sourceLang}
            onChange={(e) => setSourceLang(e.target.value)}
            disabled={recorderControls.state !== "idle"}
          >
            {LANGUAGES.map((l) => (
              <option key={l.code} value={l.code}>
                {l.label}
              </option>
            ))}
          </select>

          <button className="swap-btn" onClick={swapLanguages} disabled={recorderControls.state !== "idle"}>
            ⇄
          </button>

          <select
            className="lang-select"
            value={targetLang}
            onChange={(e) => setTargetLang(e.target.value)}
            disabled={recorderControls.state !== "idle"}
          >
            {LANGUAGES.map((l) => (
              <option key={l.code} value={l.code}>
                {l.label}
              </option>
            ))}
          </select>
        </div>

        <AudioRecorder
          state={recorderControls.state}
          analyser={recorderControls.analyser}
          onStart={recorderControls.start}
          onStop={recorderControls.stop}
        />

        {error && <div className="error">{error}</div>}

        <TranscriptionDisplay
          transcription={result?.transcription ?? ""}
          translation={result?.translation ?? ""}
          durationMs={result?.durationMs}
        />

        {result?.translation && (
          <AudioPlayer text={result.translation} />
        )}
      </main>
    </div>
  );
}
