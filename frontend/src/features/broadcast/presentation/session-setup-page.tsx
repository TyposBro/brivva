import { useCallback, useEffect, useState } from "react";
import { useNavigate, useParams } from "react-router-dom";
import { ArrowLeft, ArrowRight, Loader2 } from "lucide-react";
import { cn } from "../../../core/cn";
import * as api from "../data/api-client";
import type { VoicePreset } from "../data/api-client";
import { SignInGate } from "../../../shared/auth/sign-in-gate";
import { useAuth } from "../../../shared/auth/use-auth";
import { useVoiceRecorder } from "../../../shared/audio/voice-recorder";
import { VoicePresetPicker } from "./voice-preset-picker";
import { VoiceSetupCard } from "./voice-setup-card";
import { SOURCE_LANGS, type SourceLang } from "./source-lang-picker";
import {
  AdvancedMediaIngestSettings,
  useMediaIngestModePreference,
} from "./media-ingest-settings";

const MIN_SEC = 30;
const MAX_SEC = 180;

export default function SessionSetupPage() {
  return (
    <SignInGate>
      <SetupInner />
    </SignInGate>
  );
}

function SetupInner() {
  const { id } = useParams<{ id: string }>();
  const navigate = useNavigate();
  const userId = useAuth().userId!;

  const [session, setSession] = useState<api.Session | null>(null);
  const [streams, setStreams] = useState<api.StreamInfo[]>([]);
  const [loading, setLoading] = useState(true);
  const [cloning, setCloning] = useState(false);
  // Track whether the user has finished the voice step at least once. Seeded
  // from the session — if voice_preset != 'cloned' we treat "skip" as done.
  const [voiceReady, setVoiceReady] = useState(false);
  const [voicePreset, setVoicePreset] = useState<VoicePreset>("female");
  const [presetSaving, setPresetSaving] = useState(false);
  const [recordMode, setRecordMode] = useState(false);
  const [error, setError] = useState("");
  const [mediaIngestMode, setMediaIngestMode] = useMediaIngestModePreference();

  const reload = useCallback(async () => {
    if (!id) return;
    try {
      const data = await api.getSession(id);
      setSession(data.session);
      setStreams(data.streams);
      if (data.session) {
        const preset = (data.session.voice_preset ?? "female") as VoicePreset;
        setVoicePreset(preset);
        if (data.session.voice_id || preset !== "cloned") setVoiceReady(true);
      }
    } catch (e) {
      console.error("Failed to load session:", e);
    } finally {
      setLoading(false);
    }
  }, [id]);

  useEffect(() => {
    void reload();
  }, [reload]);

  const uploadSample = useCallback(
    async (b64: string) => {
      if (!id) return;
      setCloning(true);
      setError("");
      try {
        await api.cloneSessionVoice(id, { user_id: userId, audio_base64: b64 });
        setVoiceReady(true);
        setRecordMode(false);
        await reload();
      } catch (e) {
        setError(e instanceof Error ? e.message : "Voice clone failed");
      } finally {
        setCloning(false);
      }
    },
    [id, reload, userId],
  );

  const recorder = useVoiceRecorder({
    minSec: MIN_SEC,
    maxSec: MAX_SEC,
    onAutoStop: (b64) => void uploadSample(b64),
  });

  const handleStart = useCallback(async () => {
    try {
      await recorder.start();
    } catch (e) {
      setError(e instanceof Error ? e.message : "Microphone access denied");
    }
  }, [recorder]);

  const handleStop = useCallback(() => {
    const b64 = recorder.stop();
    if (b64 !== null) void uploadSample(b64);
  }, [recorder, uploadSample]);

  const handleSkip = useCallback(() => {
    setVoicePreset("female");
    setVoiceReady(true);
    setRecordMode(false);
  }, []);

  const handlePresetChange = useCallback(
    async (preset: VoicePreset) => {
      if (!id || preset === voicePreset) return;
      setPresetSaving(true);
      setError("");
      try {
        const res = await api.updateSessionVoicePreset(id, preset);
        setSession(res.session);
        setVoicePreset(preset);
      } catch (e) {
        setError(e instanceof Error ? e.message : "Failed to update voice preset");
      } finally {
        setPresetSaving(false);
      }
    },
    [id, voicePreset],
  );

  const goLive = () => {
    if (!id) return;
    navigate(`/session/${id}/live`);
  };

  if (loading) {
    return (
      <div className="min-h-screen bg-background flex items-center justify-center">
        <Loader2 className="w-6 h-6 text-primary animate-spin" />
      </div>
    );
  }

  if (!session) {
    return (
      <div className="min-h-screen bg-background flex flex-col items-center justify-center gap-4">
        <p className="text-error font-label">Session not found</p>
        <button
          className="bg-surface-container-high hover:bg-surface-bright text-on-surface px-5 py-2.5 rounded-lg font-headline font-bold transition-colors"
          onClick={() => navigate("/dashboard")}
        >
          Back to Dashboard
        </button>
      </div>
    );
  }

  const targetLangs = parseTargetLangs(session.target_langs);

  return (
    <div className="min-h-screen bg-background">
      <header className="fixed top-0 w-full z-50 bg-background/60 backdrop-blur-xl">
        <div className="flex justify-between items-center max-w-3xl mx-auto px-6 h-16">
          <button
            className="flex items-center gap-2 text-on-surface-variant hover:text-on-surface transition-colors font-label text-sm"
            onClick={() => navigate(`/session/${session.id}`)}
          >
            <ArrowLeft className="w-4 h-4" />
            Session
          </button>
          <h1 className="text-xl font-bold tracking-tighter text-on-surface font-headline">
            BRIVVA
          </h1>
          <span className="text-on-surface-variant font-label text-xs uppercase tracking-widest">
            Setup
          </span>
        </div>
      </header>

      <main className="max-w-3xl mx-auto px-6 pt-24 pb-16 space-y-8">
        <section>
          <h2 className="font-headline font-bold text-2xl tracking-tight text-on-surface mb-2">
            {session.title}
          </h2>
          <div className="flex flex-wrap gap-x-6 gap-y-1 text-on-surface-variant text-sm font-label">
            <span>
              Source:{" "}
              <span className="text-on-surface">{api.langLabel(session.source_lang)}</span>
            </span>
            <span>
              Targets:{" "}
              <span className="text-on-surface">
                {targetLangs.map((l) => api.langLabel(l)).join(", ") || "—"}
              </span>
            </span>
            <span>
              Streams: <span className="text-on-surface">{streams.length}</span>
            </span>
          </div>
        </section>

        {error && (
          <div className="bg-error-container/20 text-error px-4 py-2.5 rounded-lg font-label text-sm">
            {error}
          </div>
        )}

        {cloning ? (
          <div className="flex items-center justify-center gap-3 text-on-surface-variant font-label py-12">
            <Loader2 className="w-5 h-5 animate-spin text-primary" />
            Cloning your voice…
          </div>
        ) : recordMode || !session.voice_id ? (
          <VoiceSetupCard
            elapsedSec={recorder.elapsedSec}
            isRecording={recorder.isRecording}
            minSec={MIN_SEC}
            maxSec={MAX_SEC}
            onStart={handleStart}
            onStop={handleStop}
            onSkip={handleSkip}
            sourceLang={
              (SOURCE_LANGS as readonly string[]).includes(session.source_lang)
                ? (session.source_lang as SourceLang)
                : "en"
            }
          />
        ) : (
          <VoicePresetPicker
            value={voicePreset}
            onChange={(p) => void handlePresetChange(p)}
            hasClone={Boolean(session.voice_id)}
            onReRecord={() => setRecordMode(true)}
          />
        )}

        <AdvancedMediaIngestSettings
          value={mediaIngestMode}
          onChange={setMediaIngestMode}
        />

        <button
          className={cn(
            "w-full py-4 rounded-xl font-headline font-extrabold text-lg uppercase tracking-tight transition-all flex items-center justify-center gap-2",
            voiceReady && !presetSaving
              ? "monolith-gradient text-white hover:scale-[0.99] active:scale-[0.97] shadow-xl"
              : "bg-surface-container-high text-on-surface-variant cursor-not-allowed",
          )}
          disabled={!voiceReady || presetSaving}
          onClick={goLive}
        >
          Go Live
          <ArrowRight className="w-5 h-5" />
        </button>
      </main>
    </div>
  );
}

function parseTargetLangs(serialized: string): string[] {
  try {
    const parsed = JSON.parse(serialized);
    return Array.isArray(parsed) ? parsed.filter((l): l is string => typeof l === "string") : [];
  } catch (error) {
    console.warn(
      "session-setup: failed to parse session.target_langs — destination dropdowns will show empty",
      { rawLen: serialized.length, error: (error as Error).message },
    );
    return [];
  }
}
