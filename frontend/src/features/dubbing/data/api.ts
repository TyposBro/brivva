import { appConfig } from "../../../orchestration/config/app-config";
import type { DubbingJob } from "../domain/types";

const BASE = appConfig.apiBaseUrl;

type StartDubbingResponse = { job_id: string; status: string };

export async function startDubbing(
  sessionId: string,
  lang: string,
  sourceLang: string,
): Promise<{ job_id: string }> {
  const resp = await fetch(`${BASE}/api/dubbing/start`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ session_id: sessionId, lang, source_lang: sourceLang }),
  });
  if (!resp.ok) {
    const msg = await resp.text();
    throw new Error(`Failed to start dubbing: ${msg}`);
  }
  const data: StartDubbingResponse = await resp.json();
  return { job_id: data.job_id };
}

export async function getJobStatus(jobId: string): Promise<DubbingJob> {
  const resp = await fetch(`${BASE}/api/dubbing/status/${encodeURIComponent(jobId)}`);
  if (!resp.ok) throw new Error("Failed to fetch job status");
  return resp.json();
}

export async function getSessionJobs(sessionId: string): Promise<DubbingJob[]> {
  const resp = await fetch(`${BASE}/api/dubbing/jobs/${encodeURIComponent(sessionId)}`);
  if (!resp.ok) throw new Error("Failed to fetch session jobs");
  return resp.json();
}

export function getDownloadUrl(jobId: string): string {
  return `${BASE}/api/dubbing/download/${encodeURIComponent(jobId)}`;
}
