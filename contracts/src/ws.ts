import { z } from "zod";

export const HostServerInterimMessageSchema = z.object({
  type: z.literal("interim"),
  transcript: z.string(),
});

export const HostServerFinalMessageSchema = z.object({
  type: z.literal("final"),
  transcript: z.string(),
  utteranceId: z.number().int(),
  sttMs: z.number().int().optional(),
});

export const HostServerTranslationMessageSchema = z.object({
  type: z.literal("translation"),
  text: z.string(),
  utteranceId: z.number().int(),
  targetLang: z.string().min(1),
  translateMs: z.number().int().optional(),
});

export const HostServerTtsEndMessageSchema = z.object({
  type: z.literal("tts_end"),
  utteranceId: z.number().int(),
  targetLang: z.string().min(1).optional(),
  ttsMs: z.number().int().optional(),
});

export const HostServerVideoEndMessageSchema = z.object({
  type: z.literal("video_end"),
  utteranceId: z.number().int(),
  lipsyncMs: z.number().int().optional(),
});

export const HostServerErrorMessageSchema = z.object({
  type: z.literal("error"),
  message: z.string(),
});

export const HostServerProviderHealthMessageSchema = z.object({
  type: z.literal("provider_health"),
  provider: z.string().min(1),
  state: z.string().min(1),
  recoverable: z.boolean(),
  billable: z.boolean(),
  reason: z.string().min(1),
  targetLang: z.string().min(1).optional(),
  statusCode: z.number().int().optional(),
  errorCode: z.string().min(1).optional(),
  message: z.string(),
});

export const HostServerMessageSchema = z.discriminatedUnion("type", [
  HostServerInterimMessageSchema,
  HostServerFinalMessageSchema,
  HostServerTranslationMessageSchema,
  HostServerTtsEndMessageSchema,
  HostServerVideoEndMessageSchema,
  HostServerProviderHealthMessageSchema,
  HostServerErrorMessageSchema,
]);

export type HostServerMessage = z.infer<typeof HostServerMessageSchema>;
