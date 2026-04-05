import { Mic, SkipForward } from "lucide-react";

const SAMPLE_TEXT =
  "Welcome to today's live stream! I'm really excited to show you " +
  "some amazing products that I've been using lately. These items " +
  "have completely changed my daily routine, and I think you're " +
  "going to love them too. The quality is outstanding, and the " +
  "price is incredibly reasonable for what you get. I've tried " +
  "many similar products before, but nothing comes close to this. " +
  "If you have any questions, feel free to drop them in the chat " +
  "and I'll answer them right away. Let's get started!";

export function VoiceSetup({
  isVoiceRecording,
  voiceTimer,
  onStartVoice,
  onSkip,
}: {
  isVoiceRecording: boolean;
  voiceTimer: number;
  onStartVoice: () => void;
  onSkip: () => void;
}) {
  return (
    <section className="bg-surface-container-low rounded-xl p-8 max-w-lg mx-auto space-y-6">
      <div>
        <h3 className="font-headline font-bold text-2xl text-on-surface mb-2">
          Voice Setup
        </h3>
        <p className="text-on-surface-variant text-sm leading-relaxed">
          Record a 30-second voice sample to clone your voice. Read the text
          below naturally at your normal pace.
        </p>
      </div>

      {!isVoiceRecording && voiceTimer === 0 && (
        <button
          className="monolith-gradient text-white w-full py-3 rounded-xl font-headline font-bold hover:scale-[0.98] transition-all flex items-center justify-center gap-2"
          onClick={onStartVoice}
        >
          <Mic className="w-5 h-5" />
          Record Voice Sample
        </button>
      )}

      {isVoiceRecording && (
        <>
          <div className="flex items-center gap-3 text-error font-label">
            <span className="w-2 h-2 bg-error rounded-full animate-pulse" />
            Recording... {voiceTimer}s
          </div>
          <div className="bg-surface-container-highest rounded-lg p-4 text-on-surface-variant text-sm leading-relaxed font-body italic">
            {SAMPLE_TEXT}
          </div>
        </>
      )}

      <button
        className="w-full bg-surface-container-high hover:bg-surface-bright text-on-surface-variant py-2.5 rounded-lg font-label text-sm transition-colors flex items-center justify-center gap-2"
        onClick={onSkip}
      >
        <SkipForward className="w-4 h-4" />
        Skip (use default voice)
      </button>
    </section>
  );
}
