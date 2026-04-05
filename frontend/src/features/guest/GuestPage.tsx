import { useState } from "react";
import { useParams, useSearchParams } from "react-router-dom";
import { useGuestRoom } from "../../hooks/useGuestRoom";
import { type Lang, LANGS, LANG_LABELS } from "../../types";
import { LanguagePicker } from "./components/LanguagePicker";
import { GuestTranscripts } from "./components/GuestTranscripts";
import { HostVideo } from "./components/HostVideo";

export default function GuestPage() {
  const { id = "" } = useParams<{ id: string }>();
  const [searchParams] = useSearchParams();
  const urlLang = searchParams.get("lang");
  const validUrlLang = urlLang && LANGS.includes(urlLang as Lang) ? (urlLang as Lang) : null;

  const [selectedLang, setSelectedLang] = useState<Lang | null>(validUrlLang);
  const { status, liveTranscript, utterances, error, attachCanvas } = useGuestRoom(id, selectedLang);

  if (!selectedLang) {
    return <LanguagePicker roomId={id} onSelectLang={setSelectedLang} />;
  }

  return (
    <div className="app">
      <header className="header">
        <h1 className="logo">brivva</h1>
        <p className="tagline">{getStatusText(status, selectedLang)}</p>
      </header>

      <main className="main">
        {error && <div className="error">{error}</div>}

        {status === "connecting" && (
          <div className="status-bar">
            <span className="spinner" /> Joining room {id}…
          </div>
        )}

        {status === "closed" && (
          <div className="status-bar">The host has ended this session.</div>
        )}

        {status === "listening" && <HostVideo attachCanvas={attachCanvas} />}

        <GuestTranscripts
          utterances={utterances}
          liveTranscript={liveTranscript}
          selectedLang={selectedLang}
        />
      </main>
    </div>
  );
}

function getStatusText(status: string, lang: Lang): string {
  const labels: Record<string, string> = {
    connecting: "Connecting\u2026",
    listening: `Listening \u00b7 ${LANG_LABELS[lang]}`,
    closed: "Host disconnected",
    error: "Connection error",
  };
  return labels[status] ?? "";
}
