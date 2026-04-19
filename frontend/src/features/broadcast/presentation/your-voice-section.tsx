import { useCallback, useState } from "react";
import { Loader2, Mic, RefreshCw } from "lucide-react";
import * as api from "../data/api-client";
import { useVoiceRecorder } from "../../../shared/audio/voice-recorder";
import { VoiceSetupCard } from "./voice-setup-card";
import {
  SourceLangPicker,
  SOURCE_LANGS,
  detectBrowserSourceLang,
  type SourceLang,
} from "./source-lang-picker";

const MIN_SEC = 30;
const MAX_SEC = 180;

interface Props {
  userId: string;
  voice: api.Voice | null;
  defaultName: string;
  onChange: (voice: api.Voice) => void;
}

/** Narrows the persisted voice's `source_lang` (backend stores an arbitrary
 *  string or null) to one of the four voice-clone languages. Returns null when
 *  the persisted value isn't one of them, so the caller can fall back to the
 *  browser default. */
function normalizeSourceLang(raw: string | null | undefined): SourceLang | null {
  if (raw === null || raw === undefined) return null;
  return (SOURCE_LANGS as readonly string[]).includes(raw)
    ? (raw as SourceLang)
    : null;
}

// One clone per user. Workers upserts on POST /api/voices, so this card just
// records → uploads → swaps the displayed voice. There's no delete: the only
// way to remove the clone is to record over it.
export function YourVoiceSection({ userId, voice, defaultName, onChange }: Props) {
  const [recording, setRecording] = useState(false);
  const [uploading, setUploading] = useState(false);
  const [error, setError] = useState("");
  // Seed from the persisted voice when available so re-recording defaults to
  // the language the host originally cloned in; otherwise fall back to the
  // browser locale. Either way the host can override before uploading.
  const [sourceLang, setSourceLang] = useState<SourceLang>(
    () => normalizeSourceLang(voice?.source_lang) ?? detectBrowserSourceLang(),
  );
  const sourceLangFromVoice = normalizeSourceLang(voice?.source_lang) !== null;

  const upload = useCallback(
    async (b64: string) => {
      setUploading(true);
      setError("");
      try {
        const next = await api.createVoice({
          user_id: userId,
          name: voice?.name ?? defaultName,
          audio_base64: b64,
          source_lang: sourceLang,
        });
        onChange(next);
        setRecording(false);
      } catch (e) {
        setError(e instanceof Error ? e.message : "Voice upload failed");
      } finally {
        setUploading(false);
      }
    },
    [defaultName, onChange, sourceLang, userId, voice?.name],
  );

  const recorder = useVoiceRecorder({
    minSec: MIN_SEC,
    maxSec: MAX_SEC,
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

  if (recording) {
    return (
      <div className="space-y-2">
        <span className="text-on-surface-variant text-xs font-label block">
          Your voice
        </span>
        {/* Stays enabled while recording — the language is just metadata the
         *  upload carries, and hosts occasionally realise mid-recording they
         *  picked the wrong flag. Lock it only during the upload itself. */}
        <SourceLangPicker
          value={sourceLang}
          onChange={setSourceLang}
          disabled={uploading}
          showBrowserDefaultHint={!sourceLangFromVoice}
          label="What language will you record in?"
          id="dashboard-source-lang"
        />
        {uploading ? (
          <div className="flex items-center gap-2 text-on-surface-variant text-sm font-label">
            <Loader2 className="w-4 h-4 animate-spin text-primary" />
            Uploading…
          </div>
        ) : (
          <VoiceSetupCard
            elapsedSec={recorder.elapsedSec}
            isRecording={recorder.isRecording}
            minSec={MIN_SEC}
            maxSec={MAX_SEC}
            onStart={handleStart}
            onStop={handleStop}
            onSkip={() => setRecording(false)}
          />
        )}
        {error && <p className="text-error text-xs font-label">{error}</p>}
      </div>
    );
  }

  return (
    <div className="space-y-2">
      <span className="text-on-surface-variant text-xs font-label block">
        Your voice
      </span>
      {voice ? (
        <div className="flex items-center gap-3 px-3 py-2 rounded-lg bg-surface-container-high text-sm">
          <Mic className="w-3.5 h-3.5 text-primary" />
          <span className="text-on-surface font-label flex-1 truncate">
            {voice.name}
          </span>
          <button
            className="flex items-center gap-1 text-on-surface-variant hover:text-on-surface text-xs font-label transition-colors"
            onClick={() => setRecording(true)}
          >
            <RefreshCw className="w-3 h-3" />
            Re-record
          </button>
        </div>
      ) : (
        <button
          className="w-full flex items-center justify-center gap-2 bg-surface-container-high hover:bg-surface-bright text-on-surface px-3 py-2 rounded-lg text-sm font-label transition-colors"
          onClick={() => setRecording(true)}
        >
          <Mic className="w-3.5 h-3.5 text-primary" />
          Record your voice
        </button>
      )}
      {error && <p className="text-error text-xs font-label">{error}</p>}
    </div>
  );
}
