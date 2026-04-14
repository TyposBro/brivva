import { useState } from "react";
import type { DubbingJob, DubbingJobStatus } from "../../domain/types";
import { startDubbing, startAllDubbing, getDownloadUrl, cleanupSession } from "../../data/api";
import { useDubbingJobs } from "../hooks/use-dubbing-jobs";
import { LANGS } from "../../../broadcast/domain/broadcast-types";

type Props = {
  sessionId: string;
  targetLangs: string[];
  sourceLang: string;
};

// ── Pipeline steps ────────────────────────────────────────────────────────────

const STEPS = ["Preparing", "Uploading", "Dubbing", "Downloading", "Finalizing"] as const;

const STATUS_STEP: Record<DubbingJobStatus, number> = {
  pending:      -1,
  muxing:        0,
  uploading:     1,
  dubbing:       2,
  downloading:   3,
  muxing_final:  4,
  complete:      5,
  failed:       -1,
};

const STUDIO_BASE = "https://dubbing.elevenlabs.io/project";

// ── Main component ────────────────────────────────────────────────────────────

export function DubbingPanel({ sessionId, targetLangs, sourceLang }: Props) {
  const { jobs, refetch } = useDubbingJobs(sessionId);
  const [starting, setStarting] = useState<Record<string, boolean>>({});
  const [errors, setErrors] = useState<Record<string, string>>({});
  const [cleaning, setCleaning] = useState(false);

  const unstarted = targetLangs.filter((l) => !jobs.some((j) => j.lang === l));
  const allDone = jobs.length > 0 && jobs.every((j) => j.status === "complete" || j.status === "failed");

  const handleStart = async (lang: string) => {
    setStarting((p) => ({ ...p, [lang]: true }));
    setErrors((p) => { const n = { ...p }; delete n[lang]; return n; });
    try {
      await startDubbing(sessionId, lang, sourceLang);
      await refetch();
    } catch (e) {
      setErrors((p) => ({ ...p, [lang]: e instanceof Error ? e.message : String(e) }));
    } finally {
      setStarting((p) => ({ ...p, [lang]: false }));
    }
  };

  const handleStartAll = async () => {
    if (unstarted.length === 0) return;
    for (const lang of unstarted) {
      setStarting((p) => ({ ...p, [lang]: true }));
    }
    try {
      await startAllDubbing(sessionId, unstarted, sourceLang);
      await refetch();
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      for (const lang of unstarted) {
        setErrors((p) => ({ ...p, [lang]: msg }));
      }
    } finally {
      for (const lang of unstarted) {
        setStarting((p) => ({ ...p, [lang]: false }));
      }
    }
  };

  const handleCleanup = async () => {
    setCleaning(true);
    try {
      await cleanupSession(sessionId);
      await refetch();
    } catch {
      // ignore — user can retry
    } finally {
      setCleaning(false);
    }
  };

  return (
    <div className="bg-surface-container-low rounded-xl p-4 space-y-4">
      <div className="flex items-center justify-between">
        <div>
          <h2 className="font-headline text-sm font-semibold text-on-surface-variant uppercase tracking-wider">
            Post-Session Dubbing
          </h2>
          <p className="text-xs text-outline mt-0.5">
            ElevenLabs re-dubs video with translated speech (5–15 min/language)
          </p>
        </div>
        <div className="flex items-center gap-2">
          {allDone && (
            <button
              onClick={handleCleanup}
              disabled={cleaning}
              className="px-3 py-1.5 rounded-lg bg-surface-container-highest text-on-surface-variant text-xs font-semibold hover:opacity-90 transition-opacity disabled:opacity-50"
            >
              {cleaning ? "Cleaning..." : "Clean Up Files"}
            </button>
          )}
          {unstarted.length > 1 && (
            <button
              onClick={handleStartAll}
              className="px-3 py-1.5 rounded-lg bg-primary text-on-primary text-xs font-semibold hover:opacity-90 transition-opacity"
            >
              Dub All
            </button>
          )}
        </div>
      </div>

      <div className="space-y-3">
        {targetLangs.map((lang) => {
          const job = jobs.find((j) => j.lang === lang);
          const isStarting = starting[lang] ?? false;
          const startError = errors[lang];
          return (
            <LangRow
              key={lang}
              lang={lang}
              job={job}
              isStarting={isStarting}
              startError={startError}
              onStart={() => handleStart(lang)}
            />
          );
        })}
      </div>
    </div>
  );
}

// ── Per-language row ──────────────────────────────────────────────────────────

function LangRow({
  lang,
  job,
  isStarting,
  startError,
  onStart,
}: {
  lang: string;
  job?: DubbingJob;
  isStarting: boolean;
  startError?: string;
  onStart: () => void;
}) {
  const langMeta = LANGS.find((l) => l.code === lang);
  const label = langMeta ? `${langMeta.flag} ${langMeta.label}` : lang.toUpperCase();

  return (
    <div className="bg-surface-container rounded-xl p-3 space-y-2">
      <div className="flex items-center justify-between">
        <span className="text-sm font-semibold text-on-surface">{label}</span>
        <div className="flex items-center gap-2">
          {job?.dubbing_id && job.status !== "pending" && (
            <a
              href={`${STUDIO_BASE}/${job.dubbing_id}`}
              target="_blank"
              rel="noopener noreferrer"
              className="text-[10px] text-outline hover:text-primary transition-colors underline"
            >
              Studio
            </a>
          )}
          {!job && !isStarting && (
            <button
              onClick={onStart}
              className="px-3 py-1 rounded-lg bg-primary-container text-on-primary-container text-xs font-semibold hover:opacity-90 transition-opacity"
            >
              Start Dubbing
            </button>
          )}
          {isStarting && (
            <span className="text-xs text-outline flex items-center gap-1">
              <Spinner /> Starting...
            </span>
          )}
          {job?.status === "complete" && job && (
            <a
              href={getDownloadUrl(job.id)}
              download
              className="px-3 py-1 rounded-lg bg-secondary-container text-on-secondary-container text-xs font-semibold hover:opacity-90 transition-opacity"
            >
              Download MP4
            </a>
          )}
          {job?.status === "failed" && (
            <button
              onClick={onStart}
              className="px-3 py-1 rounded-lg bg-error-container text-on-error-container text-xs font-semibold hover:opacity-90 transition-opacity"
            >
              Retry
            </button>
          )}
        </div>
      </div>

      {job && job.status !== "complete" && (
        <StepProgress
          status={job.status}
          expectedDuration={job.expected_duration_sec}
          startedAt={job.started_at}
        />
      )}

      {job?.status === "complete" && (
        <p className="text-xs text-secondary font-medium">Dubbed video ready</p>
      )}

      {(job?.error || startError) && (
        <p className="text-xs text-error break-words">
          {job?.error ?? startError}
        </p>
      )}
    </div>
  );
}

// ── Step progress bar ─────────────────────────────────────────────────────────

function StepProgress({
  status,
  expectedDuration,
  startedAt,
}: {
  status: DubbingJobStatus;
  expectedDuration?: number;
  startedAt?: number;
}) {
  const activeStep = STATUS_STEP[status];
  const isFailed = status === "failed";

  const eta = computeEta(status, expectedDuration, startedAt);

  return (
    <div className="space-y-1 pt-1">
      <div className="flex items-center gap-1">
        {STEPS.map((step, i) => {
          const done = activeStep > i;
          const active = activeStep === i;
          return (
            <div key={step} className="flex-1 flex flex-col items-center gap-0.5 min-w-0">
              <div
                className={`h-1.5 w-full rounded-full transition-colors ${
                  isFailed && active ? "bg-error" :
                  done ? "bg-secondary" :
                  active ? "bg-primary animate-pulse" :
                  "bg-surface-container-highest"
                }`}
              />
              <span
                className={`text-[10px] truncate w-full text-center leading-none mt-0.5 ${
                  active && !isFailed ? "text-primary font-medium" :
                  done ? "text-outline" :
                  "text-outline/50"
                }`}
              >
                {step}
              </span>
            </div>
          );
        })}
      </div>
      {eta && (
        <p className="text-[10px] text-outline text-right">{eta}</p>
      )}
    </div>
  );
}

function computeEta(
  status: DubbingJobStatus,
  expectedDuration?: number,
  startedAt?: number,
): string | null {
  if (status !== "dubbing" || !expectedDuration || !startedAt) return null;
  const elapsed = (Date.now() / 1000) - startedAt;
  const remaining = Math.max(0, expectedDuration - elapsed);
  if (remaining < 5) return "Almost done...";
  const mins = Math.floor(remaining / 60);
  const secs = Math.floor(remaining % 60);
  return mins > 0 ? `~${mins}m ${secs}s remaining` : `~${secs}s remaining`;
}

// ── Spinner ───────────────────────────────────────────────────────────────────

function Spinner() {
  return (
    <svg
      className="animate-spin h-3 w-3 text-outline"
      xmlns="http://www.w3.org/2000/svg"
      fill="none"
      viewBox="0 0 24 24"
    >
      <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4" />
      <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8v4a4 4 0 00-4 4H4z" />
    </svg>
  );
}
