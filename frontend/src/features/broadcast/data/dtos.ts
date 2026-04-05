export const AUDIO_TAG = 0x01;
export const VIDEO_TAG = 0x02;

export type SessionCreatedMsg = { type: "session:created"; id: string };
export type InterimMsg = { type: "interim"; transcript: string };
export type FinalMsg = { type: "final"; transcript: string; utteranceId: number };
export type TranslationMsg = { type: "translation"; lang: string; text: string; utteranceId: number; translateMs: number };
export type ChunkTranslationMsg = { type: "chunk_translation"; lang: string; text: string; utteranceId: number; chunkIndex: number; translateMs: number };
export type PipelineWarningMsg = {
  type: "pipeline_warning";
  kind: string;
  lang: string;
  detail: string;
  utteranceId: number;
};
export type ErrorMsg = { type: "error"; message: string };

export type ServerMessage =
  | SessionCreatedMsg
  | InterimMsg
  | FinalMsg
  | TranslationMsg
  | ChunkTranslationMsg
  | PipelineWarningMsg
  | ErrorMsg;
