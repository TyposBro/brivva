import { useCallback, useEffect, useState } from "react";
import { useNavigate } from "react-router-dom";
import { ArrowRight, Check, Loader2, Mic, Youtube } from "lucide-react";
import { cn } from "../../../core/cn";
import { SignInGate } from "../../../shared/auth/sign-in-gate";
import { useAuth } from "../../../shared/auth/use-auth";
import { useVoiceRecorder } from "../../../shared/audio/voice-recorder";
import { VoiceSetupCard } from "./voice-setup-card";
import * as broadcastApi from "../data/api-client";
import {
  completeOnboarding,
  fetchOnboardingState,
} from "../data/onboarding-api";
import { writeDefaultTargetLang } from "../../../core/config/default-target-lang";

const STEPS = ["platform", "voice", "lang"] as const;
type Step = (typeof STEPS)[number];

const VOICE_MIN_SEC = 30;
const VOICE_MAX_SEC = 180;

export default function OnboardingPage() {
  return (
    <SignInGate>
      <Inner />
    </SignInGate>
  );
}

function Inner() {
  const navigate = useNavigate();
  const userId = useAuth().userId!;

  const [step, setStep] = useState<Step>("platform");
  const [user, setUser] = useState<broadcastApi.UserInfo | null>(null);
  const [voice, setVoice] = useState<broadcastApi.Voice | null>(null);
  const [defaultLang, setDefaultLang] = useState("en");
  const [error, setError] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [hydrated, setHydrated] = useState(false);

  // Bounce signed-in users who already finished onboarding to the dashboard.
  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const [state, info] = await Promise.all([
          fetchOnboardingState(userId),
          broadcastApi.getUser(userId),
        ]);
        if (cancelled) return;
        setUser(info);
        if (state.onboardingCompletedAt !== null) {
          navigate("/dashboard", { replace: true });
          return;
        }
      } catch {
        // Workers may not have shipped onboarding_completed_at yet — treat
        // as "not completed" and let the user run the wizard.
      } finally {
        if (!cancelled) setHydrated(true);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [navigate, userId]);

  const goNext = () => {
    const idx = STEPS.indexOf(step);
    if (idx < STEPS.length - 1) setStep(STEPS[idx + 1]);
  };

  const finish = useCallback(async () => {
    setSubmitting(true);
    setError("");
    try {
      writeDefaultTargetLang(defaultLang);
      await completeOnboarding({ userId, defaultTargetLang: defaultLang });
      navigate("/dashboard", { replace: true });
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed to complete onboarding");
    } finally {
      setSubmitting(false);
    }
  }, [defaultLang, navigate, userId]);

  if (!hydrated) {
    return (
      <div className="min-h-screen bg-background flex items-center justify-center">
        <Loader2 className="w-6 h-6 text-primary animate-spin" />
      </div>
    );
  }

  return (
    <div className="min-h-screen bg-background">
      <header className="fixed top-0 w-full z-50 bg-background/60 backdrop-blur-xl">
        <div className="flex justify-between items-center max-w-2xl mx-auto px-6 h-16">
          <h1 className="text-xl font-bold tracking-tighter text-on-surface font-headline">
            BRIVVA
          </h1>
          <span className="text-on-surface-variant font-label text-xs uppercase tracking-widest">
            Onboarding · Step {STEPS.indexOf(step) + 1} of {STEPS.length}
          </span>
        </div>
      </header>

      <main className="max-w-2xl mx-auto px-6 pt-24 pb-16 space-y-8">
        <StepDots currentStep={step} />

        {error && (
          <div className="bg-error-container/20 text-error px-4 py-2.5 rounded-lg font-label text-sm">
            {error}
          </div>
        )}

        {step === "platform" && (
          <PlatformStep
            user={user}
            userId={userId}
            onContinue={goNext}
          />
        )}

        {step === "voice" && (
          <VoiceStep
            userId={userId}
            voice={voice}
            defaultName={user?.youtube_channel_name ?? "My voice"}
            onChange={setVoice}
            onContinue={goNext}
          />
        )}

        {step === "lang" && (
          <LangStep
            value={defaultLang}
            onChange={setDefaultLang}
            onFinish={finish}
            submitting={submitting}
          />
        )}
      </main>
    </div>
  );
}

function StepDots({ currentStep }: { currentStep: Step }) {
  const idx = STEPS.indexOf(currentStep);
  return (
    <div className="flex items-center justify-center gap-2">
      {STEPS.map((_, i) => (
        <span
          key={i}
          className={cn(
            "h-1.5 rounded-full transition-all",
            i === idx ? "w-12 bg-primary" : i < idx ? "w-6 bg-success" : "w-6 bg-surface-container-highest",
          )}
        />
      ))}
    </div>
  );
}

interface PlatformStepProps {
  user: broadcastApi.UserInfo | null;
  userId: string;
  onContinue: () => void;
}

function PlatformStep({ user, userId, onContinue }: PlatformStepProps) {
  const connected = user?.youtube_connected ?? false;
  return (
    <section className="bg-surface-container-low rounded-xl p-8 space-y-6">
      <div>
        <h2 className="font-headline font-bold text-2xl text-on-surface mb-2">
          Connect a streaming platform
        </h2>
        <p className="text-on-surface-variant text-sm leading-relaxed">
          Brivva creates broadcasts on your behalf. Start with YouTube — you
          can add Grip, TikTok, Twitch, and others from the dashboard later.
        </p>
      </div>

      {connected ? (
        <div className="flex items-center gap-3 px-4 py-3 rounded-lg bg-success/10 text-success">
          <Check className="w-4 h-4" />
          <span className="font-label text-sm">
            YouTube connected
            {user?.youtube_channel_name ? ` — ${user.youtube_channel_name}` : ""}
          </span>
        </div>
      ) : (
        <a
          href={broadcastApi.youtubeAuthUrl(userId)}
          className="inline-flex items-center gap-2 bg-surface-container-high hover:bg-surface-bright text-on-surface px-4 py-2.5 rounded-lg font-label text-sm transition-colors"
        >
          <Youtube className="w-4 h-4" />
          Connect YouTube
        </a>
      )}

      <div className="flex justify-end gap-3">
        <button
          className="text-on-surface-variant hover:text-on-surface text-sm font-label"
          onClick={onContinue}
        >
          Skip for now
        </button>
        <button
          className="monolith-gradient text-white px-6 py-2.5 rounded-lg font-headline font-bold flex items-center gap-2"
          onClick={onContinue}
        >
          Continue <ArrowRight className="w-4 h-4" />
        </button>
      </div>
    </section>
  );
}

interface VoiceStepProps {
  userId: string;
  voice: broadcastApi.Voice | null;
  defaultName: string;
  onChange: (v: broadcastApi.Voice) => void;
  onContinue: () => void;
}

function VoiceStep({ userId, voice, defaultName, onChange, onContinue }: VoiceStepProps) {
  const [uploading, setUploading] = useState(false);
  const [error, setError] = useState("");

  const upload = useCallback(
    async (b64: string) => {
      setUploading(true);
      setError("");
      try {
        const next = await broadcastApi.createVoice({
          user_id: userId,
          name: voice?.name ?? defaultName,
          audio_base64: b64,
        });
        onChange(next);
      } catch (e) {
        setError(e instanceof Error ? e.message : "Voice upload failed");
      } finally {
        setUploading(false);
      }
    },
    [defaultName, onChange, userId, voice?.name],
  );

  const recorder = useVoiceRecorder({
    minSec: VOICE_MIN_SEC,
    maxSec: VOICE_MAX_SEC,
    onAutoStop: (b64) => void upload(b64),
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
    if (b64 !== null) void upload(b64);
  }, [recorder, upload]);

  return (
    <section className="space-y-4">
      <div>
        <h2 className="font-headline font-bold text-2xl text-on-surface mb-2">
          Clone your voice
        </h2>
        <p className="text-on-surface-variant text-sm leading-relaxed">
          Record at least {VOICE_MIN_SEC} seconds. Workers stores one clone
          per user — you can re-record any time from the dashboard.
        </p>
      </div>

      {error && (
        <div className="bg-error-container/20 text-error px-4 py-2.5 rounded-lg font-label text-sm">
          {error}
        </div>
      )}

      {voice ? (
        <div className="flex items-center gap-3 px-4 py-3 rounded-lg bg-success/10 text-success">
          <Mic className="w-4 h-4" />
          <span className="font-label text-sm flex-1">
            Voice cloned — {voice.name}
          </span>
        </div>
      ) : uploading ? (
        <div className="flex items-center gap-3 text-on-surface-variant font-label py-12 justify-center">
          <Loader2 className="w-5 h-5 animate-spin text-primary" />
          Uploading…
        </div>
      ) : (
        <VoiceSetupCard
          elapsedSec={recorder.elapsedSec}
          isRecording={recorder.isRecording}
          minSec={VOICE_MIN_SEC}
          maxSec={VOICE_MAX_SEC}
          onStart={handleStart}
          onStop={handleStop}
          onSkip={onContinue}
        />
      )}

      <div className="flex justify-end gap-3">
        <button
          className="text-on-surface-variant hover:text-on-surface text-sm font-label"
          onClick={onContinue}
        >
          Skip for now
        </button>
        <button
          className="monolith-gradient text-white px-6 py-2.5 rounded-lg font-headline font-bold flex items-center gap-2 disabled:opacity-50"
          onClick={onContinue}
          disabled={!voice}
        >
          Continue <ArrowRight className="w-4 h-4" />
        </button>
      </div>
    </section>
  );
}

interface LangStepProps {
  value: string;
  onChange: (lang: string) => void;
  onFinish: () => void;
  submitting: boolean;
}

function LangStep({ value, onChange, onFinish, submitting }: LangStepProps) {
  return (
    <section className="space-y-4">
      <div>
        <h2 className="font-headline font-bold text-2xl text-on-surface mb-2">
          Pick a default target language
        </h2>
        <p className="text-on-surface-variant text-sm leading-relaxed">
          We'll pre-fill new destinations with this language so you don't have
          to choose it every time.
        </p>
      </div>

      <div className="grid grid-cols-2 gap-2">
        {broadcastApi.LANGS.map((l) => (
          <button
            key={l.code}
            className={cn(
              "flex items-center gap-2 px-4 py-3 rounded-lg font-label text-sm transition-colors text-left",
              value === l.code
                ? "bg-primary-container text-on-primary-container"
                : "bg-surface-container-low hover:bg-surface-container-high text-on-surface",
            )}
            onClick={() => onChange(l.code)}
          >
            <span className="text-lg">{l.flag}</span>
            <span className="flex-1">{l.label}</span>
            {value === l.code && <Check className="w-4 h-4" />}
          </button>
        ))}
      </div>

      <button
        className="monolith-gradient w-full text-white px-6 py-3 rounded-xl font-headline font-bold flex items-center justify-center gap-2 disabled:opacity-50"
        onClick={onFinish}
        disabled={submitting}
      >
        {submitting ? (
          <>
            <Loader2 className="w-4 h-4 animate-spin" />
            Finishing…
          </>
        ) : (
          <>
            Finish <Check className="w-4 h-4" />
          </>
        )}
      </button>
    </section>
  );
}
