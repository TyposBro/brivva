import { useState } from "react";
import { useNavigate, useSearchParams } from "react-router-dom";
import { Settings2, ChevronDown, Youtube } from "lucide-react";
import { LANGS } from "../../shared/platforms";
import { deleteVoice } from "../../shared/api";
import { getUserId } from "../../shared/helpers/user-id";

import { useDashboardData } from "./hooks/useDashboardData";
import { useDestinations } from "./hooks/useDestinations";
import { useSessionCreator } from "./hooks/useSessionCreator";

import { SettingsDrawer } from "./components/SettingsDrawer";
import { PlatformPicker } from "./components/PlatformPicker";
import { DestinationCard } from "./components/DestinationCard";
import { SessionList } from "./components/SessionList";
import { GoLiveButton } from "./components/GoLiveButton";

export default function DashboardPage() {
  const navigate = useNavigate();
  const [searchParams] = useSearchParams();
  const userId = getUserId();

  // Data
  const {
    user, voices, sessions, savedCreds,
    selectedVoice, loading, setVoices, setSelectedVoice,
  } = useDashboardData(userId);

  // Session form
  const [title, setTitle] = useState("");
  const [sourceLang, setSourceLang] = useState("ko");
  const [privacyStatus, setPrivacyStatus] = useState("unlisted");
  const [showSettings, setShowSettings] = useState(false);

  // Destinations
  const {
    destinations, magicPaste,
    addDestination, removeDestination, updateDestination, handleMagicPaste,
  } = useDestinations(savedCreds, sourceLang);

  // Session creation
  const { creating, error, handleCreateSession } = useSessionCreator();

  async function handleDeleteVoice(voiceId: string) {
    await deleteVoice(voiceId);
    setVoices((prev) => prev.filter((v) => v.id !== voiceId));
    if (selectedVoice === voiceId) setSelectedVoice("");
  }

  function onGoLive() {
    handleCreateSession({
      userId, title, sourceLang, selectedVoice,
      destinations, privacyStatus, user,
    });
  }

  if (loading) {
    return (
      <div className="min-h-screen bg-background flex items-center justify-center">
        <p className="text-on-surface-variant font-label">Loading...</p>
      </div>
    );
  }

  const youtubeJustConnected = searchParams.get("youtube") === "connected";
  const hasYoutubeDest = destinations.some((d) => d.platform === "youtube");

  return (
    <div className="min-h-screen bg-background">
      <header className="fixed top-0 w-full z-50 bg-background/60 backdrop-blur-xl">
        <div className="flex justify-between items-center max-w-2xl mx-auto px-6 h-16">
          <h1
            className="text-xl font-bold tracking-tighter text-on-surface font-headline cursor-pointer"
            onClick={() => navigate("/")}
          >
            BRIVVA
          </h1>
          <button
            className="text-on-surface-variant hover:text-on-surface transition-colors p-2"
            onClick={() => setShowSettings(!showSettings)}
          >
            <Settings2 className="w-4 h-4" />
          </button>
        </div>
      </header>

      <main className="max-w-2xl mx-auto px-6 pt-24 pb-16 space-y-6">
        {youtubeJustConnected && (
          <div className="bg-success/10 text-success px-4 py-2.5 rounded-xl font-label text-sm">
            YouTube account connected
          </div>
        )}

        {showSettings && (
          <SettingsDrawer
            user={user}
            voices={voices}
            selectedVoice={selectedVoice}
            userId={userId}
            onSelectVoice={setSelectedVoice}
            onDeleteVoice={handleDeleteVoice}
            onClose={() => setShowSettings(false)}
          />
        )}

        <TitleRow title={title} sourceLang={sourceLang} onTitleChange={setTitle} onLangChange={setSourceLang} />

        <div className="space-y-3">
          {destinations.map((dest) => (
            <DestinationCard
              key={dest.uid}
              dest={dest}
              sourceLang={sourceLang}
              savedCreds={savedCreds}
              onUpdate={(patch) => updateDestination(dest.uid, patch)}
              onRemove={() => removeDestination(dest.uid)}
            />
          ))}

          <PlatformPicker
            sourceLang={sourceLang}
            magicPaste={magicPaste}
            onAddDestination={addDestination}
            onMagicPaste={handleMagicPaste}
          />
        </div>

        {error && (
          <p className="text-error text-sm font-label bg-error-container/20 px-4 py-2.5 rounded-xl">
            {error}
          </p>
        )}

        {hasYoutubeDest && (
          <YouTubePrivacy value={privacyStatus} onChange={setPrivacyStatus} />
        )}

        <GoLiveButton
          destinationCount={destinations.length}
          creating={creating}
          onClick={onGoLive}
        />

        <SessionList sessions={sessions} onSessionClick={(id) => navigate(`/session/${id}`)} />
      </main>
    </div>
  );
}

function TitleRow({
  title,
  sourceLang,
  onTitleChange,
  onLangChange,
}: {
  title: string;
  sourceLang: string;
  onTitleChange: (v: string) => void;
  onLangChange: (v: string) => void;
}) {
  return (
    <div className="flex gap-3">
      <input
        className="flex-1 min-w-0 bg-surface-container-highest border-none rounded-xl px-4 py-3 text-on-surface placeholder:text-on-surface-variant/50 focus:ring-2 focus:ring-primary/50 transition-all font-label outline-none"
        placeholder="Session title"
        value={title}
        onChange={(e) => onTitleChange(e.target.value)}
      />
      <div className="relative shrink-0">
        <select
          className="appearance-none bg-surface-container-highest border-none rounded-xl px-4 py-3 text-on-surface font-label focus:ring-2 focus:ring-primary/50 transition-all outline-none pr-9 cursor-pointer"
          value={sourceLang}
          onChange={(e) => onLangChange(e.target.value)}
        >
          {LANGS.map((l) => (
            <option key={l.code} value={l.code}>
              {l.flag} {l.label}
            </option>
          ))}
        </select>
        <ChevronDown className="absolute right-3 top-1/2 -translate-y-1/2 w-4 h-4 text-on-surface-variant pointer-events-none" />
      </div>
    </div>
  );
}

function YouTubePrivacy({
  value,
  onChange,
}: {
  value: string;
  onChange: (v: string) => void;
}) {
  return (
    <div className="flex items-center gap-3 bg-surface-container-low rounded-xl px-4 py-3">
      <Youtube className="w-4 h-4 text-[#ff0000]" />
      <span className="text-on-surface-variant text-xs font-label flex-1">
        YouTube privacy
      </span>
      <div className="relative">
        <select
          className="appearance-none bg-surface-container-highest border-none rounded-lg px-3 py-1.5 text-on-surface font-label text-sm focus:ring-2 focus:ring-primary/50 outline-none pr-7 cursor-pointer"
          value={value}
          onChange={(e) => onChange(e.target.value)}
        >
          <option value="public">Public</option>
          <option value="unlisted">Unlisted</option>
          <option value="private">Private</option>
        </select>
        <ChevronDown className="absolute right-2 top-1/2 -translate-y-1/2 w-3 h-3 text-on-surface-variant pointer-events-none" />
      </div>
    </div>
  );
}
