import { useState } from "react";
import type { DubbingJob, DubbingJobStatus } from "../../domain/types";
import { startDubbing, getDownloadUrl } from "../../data/api";
import { useDubbingJobs } from "../hooks/use-dubbing-jobs";

type Props = {
  sessionId: string;
  targetLangs: string[];
  sourceLang: string;
};

export function DubbingPanel({ sessionId, targetLangs, sourceLang }: Props) {
  const { jobs, refetch } = useDubbingJobs(sessionId);
  const [starting, setStarting] = useState<Record<string, boolean>>({});

  const handleStart = async (lang: string) => {
    setStarting((prev) => ({ ...prev, [lang]: true }));
    try {
      await startDubbing(sessionId, lang, sourceLang);
      await refetch();
    } catch {
      // Start error is visible from job status
    } finally {
      setStarting((prev) => ({ ...prev, [lang]: false }));
    }
  };

  return (
    <div className="bg-surface-container-low rounded-xl p-4 space-y-3">
      <h2 className="font-headline text-sm font-semibold text-on-surface-variant uppercase tracking-wider">
        Dubbing
      </h2>
      <div className="space-y-2">
        {targetLangs.map((lang) => (
          <LangRow
            key={lang}
            lang={lang}
            job={jobs.find((j) => j.lang === lang)}
            isStarting={starting[lang] ?? false}
            onStart={() => handleStart(lang)}
          />
        ))}
      </div>
    </div>
  );
}

function LangRow({ lang, job, isStarting, onStart }: {
  lang: string;
  job?: DubbingJob;
  isStarting: boolean;
  onStart: () => void;
}) {
  const canStart = !job && !isStarting;
  const isComplete = job?.status === "complete";

  return (
    <div className="flex items-center justify-between bg-surface-container rounded-lg px-3 py-2">
      <span className="text-sm font-medium text-on-surface uppercase w-10">{lang}</span>
      <div className="flex items-center gap-2">
        {job && <StatusBadge status={job.status} error={job.error} />}
        {canStart && (
          <button
            onClick={onStart}
            className="px-3 py-1 rounded-lg bg-primary-container text-on-primary-container text-xs font-semibold hover:opacity-90 transition-opacity"
          >
            Start Dubbing
          </button>
        )}
        {isStarting && (
          <span className="text-xs text-outline">Starting...</span>
        )}
        {isComplete && job && (
          <a
            href={getDownloadUrl(job.id)}
            download
            className="px-3 py-1 rounded-lg bg-secondary-container text-on-secondary-container text-xs font-semibold hover:opacity-90 transition-opacity"
          >
            Download
          </a>
        )}
      </div>
    </div>
  );
}

const STATUS_LABELS: Record<DubbingJobStatus, string> = {
  pending: "Pending",
  muxing: "Muxing...",
  uploading: "Uploading...",
  dubbing: "Dubbing...",
  downloading: "Downloading...",
  muxing_final: "Finalizing...",
  complete: "Complete",
  failed: "Failed",
};

const STATUS_COLORS: Record<DubbingJobStatus, string> = {
  pending: "bg-surface-container-high text-on-surface-variant",
  muxing: "bg-tertiary-container text-on-tertiary-container",
  uploading: "bg-tertiary-container text-on-tertiary-container",
  dubbing: "bg-tertiary-container text-on-tertiary-container",
  downloading: "bg-tertiary-container text-on-tertiary-container",
  muxing_final: "bg-tertiary-container text-on-tertiary-container",
  complete: "bg-secondary-container text-on-secondary-container",
  failed: "bg-error-container text-on-error-container",
};

function StatusBadge({ status, error }: { status: DubbingJobStatus; error?: string }) {
  return (
    <span
      className={`px-2 py-0.5 rounded-full text-xs font-medium ${STATUS_COLORS[status]}`}
      title={error}
    >
      {STATUS_LABELS[status]}
    </span>
  );
}
