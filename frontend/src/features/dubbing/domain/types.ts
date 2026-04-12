export type DubbingJobStatus =
  | "pending"
  | "muxing"
  | "uploading"
  | "dubbing"
  | "downloading"
  | "muxing_final"
  | "complete"
  | "failed";

export type DubbingJob = {
  id: string;
  session_id: string;
  lang: string;
  status: DubbingJobStatus;
  error?: string;
};
