import { useNavigate, useSearchParams } from "react-router-dom";
import { Settings2 } from "lucide-react";
import { deleteVoice } from "../../shared/api";
import { getUserId } from "../../shared/helpers/user-id";
import { PageLayout } from "../../shared/components/PageLayout";

import { useDashboardData } from "./hooks/useDashboardData";
import { useDashboardForm } from "./hooks/useDashboardForm";
import { useDestinations } from "./hooks/useDestinations";
import { useSessionCreator } from "./hooks/useSessionCreator";

import { SettingsDrawer } from "./components/SettingsDrawer";
import { PlatformPicker } from "./components/PlatformPicker";
import { DestinationCard } from "./components/DestinationCard";
import { SessionList } from "./components/SessionList";
import { GoLiveButton } from "./components/GoLiveButton";
import { TitleRow } from "./components/TitleRow";
import { YouTubePrivacy } from "./components/YouTubePrivacy";
import { YouTubeBanner } from "./components/YouTubeBanner";

export default function DashboardPage() {
  const navigate = useNavigate();
  const [searchParams] = useSearchParams();
  const userId = getUserId();

  const {
    user, voices, sessions, savedCreds,
    selectedVoice, loading, setVoices, setSelectedVoice,
  } = useDashboardData(userId);

  const {
    title, sourceLang, privacyStatus, showSettings,
    setTitle, setSourceLang, setPrivacyStatus, setShowSettings,
  } = useDashboardForm();

  const {
    destinations, magicPaste,
    addDestination, removeDestination, updateDestination, handleMagicPaste,
  } = useDestinations(savedCreds, sourceLang);

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
    <PageLayout
      maxWidth="2xl"
      spaceY={6}
      onTitleClick={() => navigate("/")}
      right={
        <button
          className="text-on-surface-variant hover:text-on-surface transition-colors p-2"
          onClick={() => setShowSettings(!showSettings)}
        >
          <Settings2 className="w-4 h-4" />
        </button>
      }
    >
      {youtubeJustConnected && <YouTubeBanner />}

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
    </PageLayout>
  );
}
